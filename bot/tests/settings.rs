use forex_bot::settings::Settings;
fn valid() -> String {
    "TELEGRAM_BOT_TOKEN=fake-token\nTELEGRAM_CHAT_ID=-100123\nOPENAI_API_KEY=fake-key\nOPENAI_MODEL=test-model\nOPENAI_MAX_TOKENS=1500\nDATABASE_PATH=data/bot.sqlite\nSTRATEGY_PATH=strategy/fvg-bot-rules.json\nQUOTE_STALE_SECONDS=120\nPRICE_TICK=1\n".into()
}
#[test]
fn parses_settings_without_author_allowlist_or_manual_data_gate() {
    let c = Settings::parse(&valid()).unwrap();
    assert_eq!(c.chat, -100123);
}
#[test]
fn rejects_empty_missing_duplicate_unknown_and_invalid_settings() {
    for s in [
        String::new(),
        valid().replace("fake-token", ""),
        format!("{}PRICE_TICK=1\n", valid()),
        format!("{}UNKNOWN=private-value\n", valid()),
        valid().replace("SECONDS=120", "SECONDS=0"),
    ] {
        let e = Settings::parse(&s).err().expect("must reject");
        assert!(!e.contains("private-value"));
        assert!(!e.contains("fake-token"));
    }
}
#[test]
fn quotes_are_literal_and_not_shell_expanded() {
    let c = Settings::parse(&valid().replace("test-model", "'$(do-not-run)' ")).unwrap();
    assert_eq!(c.model, "$(do-not-run)");
}
#[test]
fn global_price_tick_is_not_required_when_instrument_ticks_are_in_json() {
    assert!(Settings::parse(&valid().replace("PRICE_TICK=1\n", "")).is_ok());
}
