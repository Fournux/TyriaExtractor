use super::*;

#[test]
fn cli_rejects_legacy_extract_and_requires_snapshot() {
    let legacy = Cli::try_parse_from([
        "gwdb-extractor",
        "--extract",
        "skills",
        "--snapshot",
        "skills.snapshot",
    ])
    .expect_err("legacy top-level --extract must not parse");
    assert_eq!(legacy.kind(), clap::error::ErrorKind::UnknownArgument);

    let missing_snapshot = Cli::try_parse_from(["gwdb-extractor", "extract", "skills"])
        .expect_err("snapshot must be explicit and portable");
    assert_eq!(
        missing_snapshot.kind(),
        clap::error::ErrorKind::MissingRequiredArgument
    );
}

#[test]
fn cli_accepts_extract_subcommands_with_explicit_snapshot() {
    let skills = Cli::try_parse_from([
        "gwdb-extractor",
        "extract",
        "skills",
        "--snapshot",
        "skills.snapshot",
        "--out-dir",
        "output",
    ])
    .expect("extract skills subcommand should parse");
    assert_eq!(skills.out_dir, PathBuf::from("output"));
    match skills.command {
        Command::Extract {
            target: ExtractCommand::Skills { snapshot },
        } => assert_eq!(snapshot, PathBuf::from("skills.snapshot")),
        other => panic!("extract skills parsed as unexpected command: {other:?}"),
    }

    let items = Cli::try_parse_from([
        "gwdb-extractor",
        "extract",
        "items",
        "--snapshot",
        "items.snapshot",
        "--packet-log",
        "tyria_packets.jsonl",
        "--skip-icons",
    ])
    .expect("extract items subcommand should parse");
    match items.command {
        Command::Extract {
            target:
                ExtractCommand::Items {
                    snapshot,
                    packet_log,
                    skip_icons,
                    use_client_strings,
                    allow_unverified_capture,
                },
        } => {
            assert_eq!(snapshot, PathBuf::from("items.snapshot"));
            assert_eq!(packet_log, Some(PathBuf::from("tyria_packets.jsonl")));
            assert!(skip_icons);
            assert!(!use_client_strings);
            assert!(!allow_unverified_capture);
        }
        other => panic!("extract items parsed as unexpected command: {other:?}"),
    }

    let traced_items = Cli::try_parse_from([
        "gwdb-extractor",
        "extract",
        "items",
        "--snapshot",
        "items.snapshot",
        "--packet-log",
        "tyria_packets.jsonl",
        "--use-client-strings",
    ])
    .expect("client string opt-in should parse");
    match traced_items.command {
        Command::Extract {
            target: ExtractCommand::Items {
                use_client_strings, ..
            },
        } => assert!(use_client_strings),
        other => {
            panic!("extract items client-string opt-in parsed as unexpected command: {other:?}")
        }
    }

    let quests = Cli::try_parse_from([
        "gwdb-extractor",
        "extract",
        "quests",
        "--snapshot",
        "Gw.dat",
        "--packet-log",
        "tyria_quests.jsonl",
        "--item-log",
        "tyria_items.jsonl",
        "--allow-unverified-capture",
    ])
    .expect("extract quests subcommand should parse");
    match quests.command {
        Command::Extract {
            target:
                ExtractCommand::Quests {
                    snapshot,
                    packet_log,
                    item_log,
                    allow_unverified_capture,
                },
        } => {
            assert_eq!(snapshot, PathBuf::from("Gw.dat"));
            assert_eq!(packet_log, PathBuf::from("tyria_quests.jsonl"));
            assert_eq!(item_log, Some(PathBuf::from("tyria_items.jsonl")));
            assert!(allow_unverified_capture);
        }
        other => panic!("extract quests parsed as unexpected command: {other:?}"),
    }
}

#[test]
fn cli_accepts_remaining_public_debug_subcommands() {
    let dump_entries =
        Cli::try_parse_from(["gwdb-extractor", "dump-entries", "--gw-dat", "Gw.dat"])
            .expect("dump-entries command should parse");
    match dump_entries.command {
        Command::DumpEntries { gw_dat, .. } => assert_eq!(gw_dat, PathBuf::from("Gw.dat")),
        other => panic!("dump-entries parsed as unexpected command: {other:?}"),
    }

    let extract_entry = Cli::try_parse_from([
        "gwdb-extractor",
        "extract-entry",
        "--gw-dat",
        "Gw.dat",
        "--index",
        "42",
    ])
    .expect("extract-entry command should parse");
    match extract_entry.command {
        Command::ExtractEntry { gw_dat, index, .. } => {
            assert_eq!(gw_dat, PathBuf::from("Gw.dat"));
            assert_eq!(index, 42);
        }
        other => panic!("extract-entry parsed as unexpected command: {other:?}"),
    }
}

#[test]
fn cli_rejects_item_extraction_without_work() {
    let error = Cli::try_parse_from([
        "gwdb-extractor",
        "extract",
        "items",
        "--snapshot",
        "items.snapshot",
        "--skip-icons",
    ])
    .expect_err("--skip-icons without --packet-log must fail");
    assert_eq!(
        error.kind(),
        clap::error::ErrorKind::MissingRequiredArgument
    );
}

#[test]
fn cli_rejects_removed_debug_subcommands_and_renders_help() {
    let help = Cli::try_parse_from(["gwdb-extractor", "help"])
        .expect_err("help subcommand should render clap help instead of running");
    assert_eq!(help.kind(), clap::error::ErrorKind::DisplayHelp);

    for removed in [
        "export-model-file-icons",
        "export-item-image-corpus",
        "link-item-image-corpus",
        "canonical-item-icons",
    ] {
        let err = Cli::try_parse_from(["gwdb-extractor", removed])
            .expect_err("removed debug subcommand must not parse");
        assert_eq!(err.kind(), clap::error::ErrorKind::InvalidSubcommand);
    }
}
