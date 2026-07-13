use super::*;

pub(super) fn for_each_packet_log_row(
    packet_log_path: &Path,
    mut visit: impl FnMut(serde_json::Value),
) -> Result<()> {
    let file = File::open(packet_log_path)
        .with_context(|| format!("opening packet log {}", packet_log_path.display()))?;
    for (line_index, line) in BufReader::new(file).lines().enumerate() {
        let line = line.with_context(|| {
            format!(
                "reading packet log {} line {}",
                packet_log_path.display(),
                line_index + 1
            )
        })?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let row = serde_json::from_str(line).with_context(|| {
            format!(
                "parsing packet log {} line {}",
                packet_log_path.display(),
                line_index + 1
            )
        })?;
        visit(row);
    }
    Ok(())
}
