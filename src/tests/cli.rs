use super::*;

#[test]
fn cli_accepts_top_level_extract_forms_with_explicit_snapshot() {
    let skills = Cli::try_parse_from([
        "gwdb-extractor",
        "--extract",
        "skills",
        "--snapshot",
        "skills.snapshot",
    ])
    .expect("top-level --extract skills should parse");
    assert!(matches!(skills.extract, Some(ExtractTarget::Skills)));
    assert!(
        skills.command.is_none(),
        "top-level --extract skills must not require a subcommand"
    );
    assert_eq!(skills.snapshot, PathBuf::from("skills.snapshot"));

    let items = Cli::try_parse_from([
        "gwdb-extractor",
        "--extract",
        "items",
        "--snapshot",
        "items.snapshot",
    ])
    .expect("top-level --extract items should parse");
    assert!(matches!(items.extract, Some(ExtractTarget::Items)));
    assert!(
        items.command.is_none(),
        "top-level --extract items must not require a subcommand"
    );
    assert_eq!(items.snapshot, PathBuf::from("items.snapshot"));
}

#[test]
fn cli_accepts_extract_subcommands_with_explicit_snapshot() {
    let skills = Cli::try_parse_from([
        "gwdb-extractor",
        "extract",
        "skills",
        "--snapshot",
        "skills.snapshot",
    ])
    .expect("extract skills subcommand should parse");
    assert!(skills.extract.is_none());
    match skills.command {
        Some(Command::Extract {
            target: ExtractCommand::Skills { snapshot },
        }) => assert_eq!(snapshot, PathBuf::from("skills.snapshot")),
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
    assert!(items.extract.is_none());
    match items.command {
        Some(Command::Extract {
            target:
                ExtractCommand::Items {
                    snapshot,
                    packet_log,
                    skip_icons,
                    use_client_strings,
                },
        }) => {
            assert_eq!(snapshot, PathBuf::from("items.snapshot"));
            assert_eq!(packet_log, Some(PathBuf::from("tyria_packets.jsonl")));
            assert!(skip_icons);
            assert!(!use_client_strings);
        }
        other => panic!("extract items parsed as unexpected command: {other:?}"),
    }

    let traced_items = Cli::try_parse_from([
        "gwdb-extractor",
        "extract",
        "items",
        "--packet-log",
        "tyria_packets.jsonl",
        "--use-client-strings",
    ])
    .expect("client string opt-in should parse");
    match traced_items.command {
        Some(Command::Extract {
            target: ExtractCommand::Items {
                use_client_strings, ..
            },
        }) => assert!(use_client_strings),
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
    ])
    .expect("extract quests subcommand should parse");
    match quests.command {
        Some(Command::Extract {
            target:
                ExtractCommand::Quests {
                    snapshot,
                    packet_log,
                    item_log,
                },
        }) => {
            assert_eq!(snapshot, PathBuf::from("Gw.dat"));
            assert_eq!(packet_log, PathBuf::from("tyria_quests.jsonl"));
            assert_eq!(item_log, Some(PathBuf::from("tyria_items.jsonl")));
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
        Some(Command::DumpEntries { gw_dat, .. }) => assert_eq!(gw_dat, PathBuf::from("Gw.dat")),
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
        Some(Command::ExtractEntry { gw_dat, index, .. }) => {
            assert_eq!(gw_dat, PathBuf::from("Gw.dat"));
            assert_eq!(index, 42);
        }
        other => panic!("extract-entry parsed as unexpected command: {other:?}"),
    }
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
