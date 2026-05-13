// App shell assets and shared card styling.
//
// This file is included by `app.rs` at module scope. It intentionally keeps
// the global CSS, injected map scripts, and small route-shell helpers together
// because all route pages use the same visual frame.

// --- global css ---
const GLOBAL_CSS: &str = r#"
:root {
    --gs26-app-height: 100dvh;
    --gs26-app-background: #020617;
    --gs26-app-text: #e5e7eb;
    --gs26-panel-background: #0b1220;
    --gs26-panel-alt-background: #0f172a;
    --gs26-border: #334155;
    --gs26-text-muted: #94a3b8;
    --gs26-text-secondary: #cbd5e1;
    --gs26-button-background: #111827;
    --gs26-button-text: #e5e7eb;
}

@supports not (height: 100dvh) {
    :root {
        --gs26-app-height: 100vh;
    }
}

html, body {
    margin: 0;
    padding: 0;
    width: 100%;
    min-height: var(--gs26-app-height);
    height: var(--gs26-app-height);
    background: var(--gs26-app-background);
    color: var(--gs26-app-text);
    overflow: hidden;
}

:root, html {
    color-scheme: dark;
}

#main {
    width: 100%;
    min-height: var(--gs26-app-height);
    height: var(--gs26-app-height);
    background: var(--gs26-app-background);
    color: var(--gs26-app-text);
}

* { box-sizing: border-box; }
"#;

const _CONNECT_SHOWN_KEY: &str = "gs_connect_shown";

fn shell_theme() -> ThemeConfig {
    telemetry_dashboard::app_shell_theme()
}

#[cfg(target_arch = "wasm32")]
fn ensure_document_text_node(tag: &str, id: &str, text: &str) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(document) = window.document() else {
        return;
    };
    let Some(head) = document.head() else {
        return;
    };

    if let Some(existing) = document.get_element_by_id(id) {
        if existing.text_content().as_deref() != Some(text) {
            existing.set_text_content(Some(text));
        }
        return;
    }

    let Ok(node) = document.create_element(tag) else {
        return;
    };
    let _ = node.set_attribute("id", id);
    node.set_text_content(Some(text));
    let _ = head.append_child(&node);
}

#[cfg(target_arch = "wasm32")]
fn ensure_document_inline_script(id: &str, text: &str) {
    ensure_document_text_node("script", id, text);
}

#[cfg(target_arch = "wasm32")]
fn ensure_document_inline_style(id: &str, text: &str) {
    ensure_document_text_node("style", id, text);
}

fn shell_page_style(theme: &ThemeConfig) -> String {
    format!(
        "min-height:var(--gs26-app-height); height:var(--gs26-app-height); overflow-y:auto; overflow-x:hidden; display:flex; align-items:center; justify-content:center; background:{}; color:{}; font-family:system-ui, -apple-system, BlinkMacSystemFont;",
        theme.app_background, theme.text_primary
    )
}

fn shell_card_style(theme: &ThemeConfig, width: &str) -> String {
    format!(
        "width:{width}; padding:24px; border:1px solid {}; border-radius:16px; background:{}; color:{}; box-shadow:0 12px 30px rgba(0,0,0,0.34);",
        theme.border_strong, theme.panel_background, theme.text_primary
    )
}

fn shell_button_style(theme: &ThemeConfig) -> String {
    format!(
        "padding:10px 14px; border-radius:12px; border:1px solid {}; background:{}; color:{}; font-family:system-ui, -apple-system, BlinkMacSystemFont; cursor:pointer;",
        theme.button_border, theme.button_background, theme.button_text
    )
}

fn shell_button_alt_style(theme: &ThemeConfig) -> String {
    format!(
        "padding:10px 14px; border-radius:12px; border:1px solid {}; background:{}; color:{}; font-family:system-ui, -apple-system, BlinkMacSystemFont; cursor:pointer;",
        theme.tab_shell_border, theme.panel_background_alt, theme.text_primary
    )
}

fn shell_input_style(theme: &ThemeConfig, margin_bottom: bool) -> String {
    format!(
        "width:100%; padding:12px; border-radius:12px; border:1px solid {}; background:{}; color:{}; outline:none;{}",
        theme.border,
        theme.app_background,
        theme.text_primary,
        if margin_bottom {
            " margin-bottom:12px;"
        } else {
            ""
        }
    )
}

fn shell_notice_style(theme: &ThemeConfig) -> String {
    format!(
        "margin-top:14px; padding:12px; border-radius:12px; border:1px solid {}; background:{}; color:{}; white-space:pre-wrap; overflow-wrap:anywhere; word-break:break-word; line-height:1.4; max-width:72ch; align-self:flex-start;",
        theme.border, theme.app_background, theme.text_secondary
    )
}

fn shell_warning_style(theme: &ThemeConfig) -> String {
    format!(
        "margin-bottom:14px; padding:12px; border-radius:12px; border:1px solid {}; background:{}; color:{}; white-space:pre-wrap; overflow-wrap:anywhere; word-break:break-word;",
        theme.warning_border, theme.warning_background, theme.warning_text
    )
}

fn format_session_load_error(err: &str) -> String {
    let lower = err.to_ascii_lowercase();
    let tls_like = lower.contains("ssl")
        || lower.contains("tls")
        || lower.contains("certificate")
        || lower.contains("unknown issuer")
        || lower.contains("self signed")
        || lower.contains("invalid peer certificate");

    if tls_like {
        err.to_string()
    } else {
        format!(
            "{}\n\nThe app could not load the Ground Station session endpoint. Check that the Ground Station URL is correct and that the proxy or server is healthy.",
            err
        )
    }
}
