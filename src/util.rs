#[cfg_attr(not(feature = "vapoursynth"), expect(dead_code))]
pub(crate) fn escape_python_string(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\\' => r"\\".to_string(),
            '"' => r#"\""#.to_string(),
            '\n' => r"\n".to_string(),
            '\r' => r"\r".to_string(),
            '\t' => r"\t".to_string(),
            c => c.to_string(),
        })
        .collect()
}
