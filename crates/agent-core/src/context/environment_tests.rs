use super::*;

#[test]
fn renders_codex_style_environment_context() {
    let context = EnvironmentContext::new(
        r"D:\learn\gifti\cloudagent",
        "powershell",
        "2026-04-30",
        "19:16:01",
        "2026-04-30T19:16:01+08:00",
        "+08:00",
    );

    assert_eq!(
        context.render_text(),
        "<environment_context>\n  <cwd>D:\\learn\\gifti\\cloudagent</cwd>\n  <shell>powershell</shell>\n  <current_date>2026-04-30</current_date>\n  <current_time>19:16:01</current_time>\n  <current_datetime>2026-04-30T19:16:01+08:00</current_datetime>\n  <timezone>+08:00</timezone>\n</environment_context>"
    );
}
