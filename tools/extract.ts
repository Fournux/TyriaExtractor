import { existsSync, readdirSync } from "node:fs";
import { join, resolve } from "node:path";

const root = process.cwd();
const gwDat = resolve(Bun.env.GW_DAT ?? "C:/Program Files (x86)/Guild Wars/Gw.dat");
const args = Bun.argv.slice(2);
const dryRun = args.includes("--dry-run");
const requestedTarget = args.find((arg) => !arg.startsWith("--")) ?? "all";
let captureDir: string | undefined;

if (!existsSync(gwDat)) {
  throw new Error(`Gw.dat not found: ${gwDat} (set GW_DAT to use another path)`);
}

function latestCaptureDir(): string {
  const configured = Bun.env.CAPTURE_DIR;
  if (configured) {
    return resolve(root, /^\d+$/.test(configured) ? join("captures", configured) : configured);
  }

  const capturesRoot = join(root, "captures");
  const session = readdirSync(capturesRoot, { withFileTypes: true })
    .filter((entry) => entry.isDirectory() && /^\d+$/.test(entry.name))
    .map((entry) => entry.name)
    .sort((left, right) => right.localeCompare(left))[0];

  if (!session) {
    throw new Error(`No capture found in ${capturesRoot}`);
  }
  return join(capturesRoot, session);
}

function captureFile(name: string, required = true): string | undefined {
  captureDir ??= latestCaptureDir();
  const path = join(captureDir, name);
  if (existsSync(path)) return path;
  if (required) throw new Error(`Required capture stream not found: ${path}`);
  return undefined;
}

function base(target: string): string[] {
  return [
    "cargo",
    "run",
    "--release",
    "-p",
    "tyria-extractor-rs",
    "--",
    "extract",
    target,
    "--snapshot",
    gwDat,
  ];
}

const extractors: Record<string, () => string[]> = {
  skills: () => base("skills"),
  images: () => base("images"),
  items: () => [...base("items"), "--packet-log", captureFile("tyria_items.jsonl")!],
  quests: () => {
    const itemLog = captureFile("tyria_items.jsonl", false);
    return [
      ...base("quests"),
      "--packet-log",
      captureFile("tyria_npcs.jsonl")!,
      "--packet-log",
      captureFile("tyria_quests.jsonl")!,
      ...(itemLog ? ["--item-log", itemLog] : []),
    ];
  },
  npcs: () => {
    const collectors = captureFile("tyria_collectors.jsonl", false);
    return [
      ...base("npcs"),
      "--packet-log",
      captureFile("tyria_npcs.jsonl")!,
      ...(collectors ? ["--packet-log", collectors] : []),
    ];
  },
  vendors: () => {
    const logs = [
      "tyria_npcs.jsonl",
      "tyria_vendor_context.jsonl",
      "tyria_collectors.jsonl",
      "tyria_merchants.jsonl",
      "tyria_crafters.jsonl",
      "tyria_skill_trainers.jsonl",
    ]
      .map((name) => captureFile(name, false))
      .filter((path): path is string => path !== undefined);
    if (!logs.length) throw new Error(`No vendor capture stream found in ${captureDir}`);
    return [...base("vendors"), ...logs.flatMap((path) => ["--packet-log", path])];
  },
};

const targets =
  requestedTarget === "all"
    ? ["skills", "images", "items", "quests", "npcs", "vendors"]
    : [requestedTarget];

for (const target of targets) {
  const command = extractors[target]?.();
  if (!command) {
    throw new Error(`Unknown target: ${target}. Choose from: all, ${Object.keys(extractors).join(", ")}`);
  }

  console.log(`\n> ${command.map((arg) => (arg.includes(" ") ? JSON.stringify(arg) : arg)).join(" ")}`);
  if (dryRun) continue;

  const result = Bun.spawnSync(command, {
    cwd: root,
    stdin: "inherit",
    stdout: "inherit",
    stderr: "inherit",
  });
  if (!result.success) process.exit(result.exitCode);
}
