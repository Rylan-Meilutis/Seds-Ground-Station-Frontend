use super::BlinkMode;

pub(crate) const ACTION_BLINK_CSS: &str = r#"
@keyframes gs26-blink-slow {
  0% { opacity: 0.2; }
  50% { opacity: 1.0; }
  100% { opacity: 0.2; }
}
@keyframes gs26-blink-slow-invert {
  0% { opacity: 1.0; }
  50% { opacity: 0.25; }
  100% { opacity: 1.0; }
}
@keyframes gs26-blink-fast {
  0% { opacity: 0.15; }
  50% { opacity: 1.0; }
  100% { opacity: 0.15; }
}
@keyframes gs26-blink-fast-invert {
  0% { opacity: 1.0; }
  50% { opacity: 0.2; }
  100% { opacity: 1.0; }
}
"#;

pub(crate) fn action_animation_style(
    enabled: bool,
    blink: BlinkMode,
    actuated: Option<bool>,
) -> &'static str {
    if !enabled {
        return "";
    }
    match (blink, actuated.unwrap_or(false)) {
        (BlinkMode::None, _) => "",
        (BlinkMode::Slow, false) => "animation:gs26-blink-slow 1800ms ease-in-out infinite;",
        (BlinkMode::Slow, true) => "animation:gs26-blink-slow-invert 1800ms ease-in-out infinite;",
        (BlinkMode::Fast, false) => "animation:gs26-blink-fast 600ms ease-in-out infinite;",
        (BlinkMode::Fast, true) => "animation:gs26-blink-fast-invert 600ms ease-in-out infinite;",
    }
}
