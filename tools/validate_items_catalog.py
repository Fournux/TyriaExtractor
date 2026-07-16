#!/usr/bin/env python3
"""Validate and report coverage for the generated runtime items catalog."""

from __future__ import annotations

import argparse
import json
from collections import defaultdict
from pathlib import Path


LANGUAGES = ("en", "ko", "fr", "de", "it", "es", "zh_tw", "zh_cn", "ja", "pl", "ru")
REQUIRED_FIELDS = (
    "model_id",
    "model_file_id",
    "packet_name_id",
    "item_type",
    "materials",
)


def identity(item: dict) -> dict:
    return {
        key: item.get(key)
        for key in ("model_id", "model_file_id", "item_ids", "packet_name_id")
        if key in item
    }


def validate(path: Path) -> dict:
    items = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(items, list):
        raise ValueError("items catalog must be a top-level JSON array")

    errors: list[str] = []
    pairs: set[tuple[int, int]] = set()
    model_files_by_model: defaultdict[int, set[int]] = defaultdict(set)
    missing_names: list[dict] = []
    missing_descriptions: list[dict] = []
    unavailable_descriptions: list[dict] = []
    partial_names: list[dict] = []
    partial_descriptions: list[dict] = []
    named_rows = described_rows = fully_named_rows = fully_described_rows = 0

    for index, item in enumerate(items):
        if not isinstance(item, dict):
            errors.append(f"row {index}: expected object")
            continue
        for field in REQUIRED_FIELDS:
            if field not in item:
                errors.append(f"row {index}: missing {field}")

        model_id = item.get("model_id")
        model_file_id = item.get("model_file_id")
        if isinstance(model_id, int) and isinstance(model_file_id, int):
            pair = (model_id, model_file_id)
            if pair in pairs:
                errors.append(f"row {index}: duplicate identity {pair}")
            pairs.add(pair)
            model_files_by_model[model_id].add(model_file_id)

        item_ids = item.get("item_ids")
        if item_ids is not None and (
            not isinstance(item_ids, list)
            or not item_ids
            or not all(isinstance(value, int) for value in item_ids)
        ):
            errors.append(f"row {index}: item_ids must be a non-empty integer array")

        name_values = [item.get(f"name_{language}") for language in LANGUAGES]
        description_values = [
            item.get(f"description_{language}") for language in LANGUAGES
        ]
        present_name_languages = [
            language
            for language, value in zip(LANGUAGES, name_values)
            if isinstance(value, str) and value
        ]
        present_description_languages = [
            language
            for language, value in zip(LANGUAGES, description_values)
            if isinstance(value, str) and value
        ]
        has_name = bool(present_name_languages)
        has_description = bool(present_description_languages)
        if has_name:
            named_rows += 1
            if len(present_name_languages) != len(LANGUAGES):
                partial_names.append(
                    identity(item)
                    | {
                        "missing_languages": [
                            language
                            for language in LANGUAGES
                            if language not in present_name_languages
                        ]
                    }
                )
        else:
            missing_names.append(identity(item))
        if has_description:
            described_rows += 1
            if len(present_description_languages) != len(LANGUAGES):
                partial_descriptions.append(
                    identity(item)
                    | {
                        "missing_languages": [
                            language
                            for language in LANGUAGES
                            if language not in present_description_languages
                        ]
                    }
                )
        elif item.get("runtime_description_available") is False:
            unavailable_descriptions.append(identity(item))
        else:
            missing_descriptions.append(identity(item))
        fully_named_rows += len(present_name_languages) == len(LANGUAGES)
        fully_described_rows += len(present_description_languages) == len(LANGUAGES)

    shared_model_ids = {
        str(model_id): sorted(model_file_ids)
        for model_id, model_file_ids in model_files_by_model.items()
        if len(model_file_ids) > 1
    }
    report = {
        "path": str(path),
        "rows": len(items),
        "unique_model_ids": len(model_files_by_model),
        "unique_model_file_ids": len(
            {model_file_id for model_file_ids in model_files_by_model.values() for model_file_id in model_file_ids}
        ),
        "languages": list(LANGUAGES),
        "named_rows": named_rows,
        "fully_named_rows": fully_named_rows,
        "described_rows": described_rows,
        "fully_described_rows": fully_described_rows,
        "missing_names": missing_names,
        "missing_descriptions": missing_descriptions,
        "runtime_unavailable_descriptions": unavailable_descriptions,
        "partial_names": partial_names,
        "partial_descriptions": partial_descriptions,
        "shared_model_ids": shared_model_ids,
        "errors": errors,
    }
    report["complete"] = (
        not errors
        and fully_named_rows == len(items)
        and fully_described_rows + len(unavailable_descriptions) == len(items)
    )
    return report


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("path", type=Path, nargs="?", default=Path("items/items.json"))
    parser.add_argument("--out", type=Path)
    parser.add_argument("--require-complete", action="store_true")
    args = parser.parse_args()

    report = validate(args.path)
    encoded = json.dumps(report, ensure_ascii=False, indent=2) + "\n"
    if args.out:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(encoded, encoding="utf-8")
    else:
        print(encoded, end="")

    if report["errors"] or (args.require_complete and not report["complete"]):
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
