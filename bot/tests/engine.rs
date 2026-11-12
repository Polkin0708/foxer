use forex_bot::{engine::*, trading::*};
fn cfg() -> forex_bot::model::Strategy {
    let mut c = forex_bot::load(std::path::Path::new("../strategy/fvg-bot-rules.json")).unwrap();
    c.parameters.atr_period.value = 1;
    c.context.minimum_bars.w = 1;
    c.context.minimum_bars.d = 1;
    c.context.minimum_bars.h4 = 1;
    c
}
fn bar(t: i64, o: i64, h: i64, l: i64, c: i64) -> Candle {
    Candle {
        end: t,
        open: o,
        high: h,
        low: l,
        close: c,
    }
}
fn poi(t: i64) -> Zone {
    Zone::new(Side::Long, 90, 100, t, ZoneKind::Fvg)
}
#[test]
fn seed_creates_context_without_historical_signals_and_replay_is_idempotent() {
    let c = cfg();
    let mut e = Engine::default();
    let bars = [
        bar(1, 100, 110, 90, 105),
        bar(2, 105, 125, 100, 120),
        bar(3, 125, 135, 120, 130),
    ];
    for b in &bars {
        e.seed(Timeframe::W, b.clone(), &c).unwrap();
    }
    assert_eq!(e.trend(Timeframe::W), Some(Side::Long));
    assert_eq!(e.plan_count(), 0);
    let before = serde_json::to_string(&e).unwrap();
    e.seed(Timeframe::W, bars[2].clone(), &c).unwrap();
    assert_eq!(serde_json::to_string(&e).unwrap(), before);
    let mut changed = bars[2].clone();
    changed.close = 129;
    assert!(e.seed(Timeframe::W, changed, &c).is_err());
}
#[test]
fn two_distinct_setups_are_kept_and_same_setup_is_not_duplicated() {
    let mut e = Engine::default();
    assert!(e.add_plan("W_point_A", Side::Long, Timeframe::W, poi(1), 10));
    assert!(!e.add_plan("W_point_A", Side::Long, Timeframe::W, poi(1), 10));
    assert!(e.add_plan("W_point_A", Side::Long, Timeframe::W, poi(2), 10));
    assert_eq!(e.plan_count(), 2);
    let e: Engine = serde_json::from_str(&serde_json::to_string(&e).unwrap()).unwrap();
    assert_eq!(e.plan_count(), 2);
}
#[test]
fn weekly_poi_prefers_closer_daily_target_only_with_sufficient_rr() {
    let c = cfg();
    let mut e = Engine::default();
    e.seed(Timeframe::W, bar(1, 180, 200, 170, 180), &c)
        .unwrap();
    e.seed(Timeframe::W, bar(2, 180, 210, 170, 180), &c)
        .unwrap();
    e.seed(Timeframe::W, bar(3, 180, 200, 170, 180), &c)
        .unwrap();
    e.seed(Timeframe::D, bar(1, 140, 150, 130, 140), &c)
        .unwrap();
    e.seed(Timeframe::D, bar(2, 140, 160, 130, 140), &c)
        .unwrap();
    e.seed(Timeframe::D, bar(3, 140, 150, 130, 140), &c)
        .unwrap();
    assert_eq!(e.target(Side::Long, Timeframe::W, 100, 90, &c), Some(160));
    assert_eq!(e.target(Side::Long, Timeframe::W, 150, 140, &c), Some(210));
    assert_eq!(e.target(Side::Long, Timeframe::D, 100, 90, &c), Some(160));
}
#[test]
fn no_weekly_direction_does_not_create_daily_plans() {
    let c = cfg();
    let mut e = Engine::default();
    for b in [
        bar(1, 100, 110, 90, 100),
        bar(2, 100, 120, 90, 110),
        bar(3, 130, 140, 125, 135),
    ] {
        e.closed(Timeframe::D, b, &c).unwrap();
    }
    assert_eq!(e.plan_count(), 0);
}
#[test]
fn expired_plans_do_not_enter_later() {
    let c = cfg();
    let mut e = Engine::default();
    e.add_plan("W_point_A", Side::Long, Timeframe::W, poi(1), 10);
    e.quote(
        Quote {
            time: 10 + 11 * 86400,
            bid: 100,
            ask: 101,
        },
        1,
        &c,
    )
    .unwrap();
    assert_eq!(e.waiting_count(), 0);
    assert!(e.signals().is_empty());
}
#[test]
fn weekly_zone_touch_creates_plan_and_weekly_end_cancels_waiting_only() {
    let c = cfg();
    let mut e = Engine::default();
    for b in [
        bar(1, 100, 110, 90, 105),
        bar(2, 105, 125, 100, 120),
        bar(3, 125, 135, 120, 130),
    ] {
        e.seed(Timeframe::W, b, &c).unwrap();
    }
    e.closed(Timeframe::D, bar(4, 115, 119, 112, 118), &c)
        .unwrap();
    assert_eq!(e.waiting_count(), 1);
    e.closed(Timeframe::W, bar(5, 130, 135, 90, 95), &c)
        .unwrap();
    assert_eq!(e.trend(Timeframe::W), None);
    assert_eq!(e.waiting_count(), 0);
}
#[test]
fn daily_inversion_creates_daily_correction_poi() {
    let c = cfg();
    let mut e = Engine::default();
    for tf in [Timeframe::W, Timeframe::D] {
        for b in [
            bar(1, 100, 110, 90, 105),
            bar(2, 105, 125, 100, 120),
            bar(3, 125, 135, 120, 130),
        ] {
            e.seed(tf, b, &c).unwrap();
        }
    }
    e.closed(Timeframe::D, bar(4, 130, 135, 90, 95), &c)
        .unwrap();
    let snapshot = serde_json::to_value(&e).unwrap();
    let p = snapshot["plans"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["rule"] == "D_correction_start")
        .expect("correction plan");
    assert_eq!(p["poi_tf"], "D");
    assert_eq!(p["side"], "Short");
}

#[test]
fn all_daily_rules_reject_non_daily_poi_without_mutating_state() {
    for rule in [
        "D_correction_start",
        "D_correction_end",
        "D_continuation_from_weekly_zone",
        "D_correction_after_weekly_fractal",
    ] {
        let mut e = Engine::default();
        let before = serde_json::to_value(&e).unwrap();
        for tf in [Timeframe::W, Timeframe::H4] {
            assert!(
                !e.add_plan(rule, Side::Long, tf, poi(1), 10),
                "{rule}: {tf:?}"
            );
            assert_eq!(serde_json::to_value(&e).unwrap(), before);
        }
        assert!(e.add_plan(rule, Side::Long, Timeframe::D, poi(1), 10));
        let restored: Engine = serde_json::from_value(serde_json::to_value(&e).unwrap()).unwrap();
        assert_eq!(
            serde_json::to_value(&restored).unwrap()["plans"][0]["poi_tf"],
            "D"
        );
    }
}

#[test]
fn weekly_point_a_rejects_daily_and_h4_poi() {
    let mut e = Engine::default();
    for tf in [Timeframe::D, Timeframe::H4] {
        assert!(!e.add_plan("W_point_A", Side::Long, tf, poi(1), 10));
    }
    assert_eq!(e.plan_count(), 0);
    assert!(e.add_plan("W_point_A", Side::Long, Timeframe::W, poi(1), 10));
}

#[test]
fn daily_target_does_not_fall_back_to_weekly_structure() {
    let c = cfg();
    let mut e = Engine::default();
    for b in [
        bar(1, 140, 150, 130, 140),
        bar(2, 140, 160, 130, 140),
        bar(3, 140, 150, 130, 140),
    ] {
        e.seed(Timeframe::W, b, &c).unwrap();
    }
    assert_eq!(e.target(Side::Long, Timeframe::D, 100, 90, &c), None);
    for b in [
        bar(1, 180, 200, 170, 180),
        bar(2, 180, 210, 170, 180),
        bar(3, 180, 200, 170, 180),
    ] {
        e.seed(Timeframe::D, b, &c).unwrap();
    }
    assert_eq!(e.target(Side::Long, Timeframe::D, 100, 90, &c), Some(210));
}

#[test]
fn stale_revision_preserves_levels_and_cannot_resurrect_result() {
    let c = cfg();
    let mut e = Engine::default();
    // A persisted published signal can outlive its original plan.
    let s = Signal::new(
        "published",
        Side::Long,
        Quote {
            time: 1,
            bid: 100,
            ask: 101,
        },
        90,
        150,
        1,
        &c,
    )
    .unwrap();
    let mut snapshot = serde_json::to_value(&e).unwrap();
    snapshot["signals"] = serde_json::json!([s]);
    e = serde_json::from_value(snapshot).unwrap();
    let mut revised = e.signals()[0].clone();
    revised.stop = 95;
    revised.target = 160;
    revised.take_profit = 160;
    assert_eq!(e.signals()[0].stop, 90);
    assert!(!e.activate_revision(&revised).unwrap());
    assert_eq!(e.signals()[0].stop, 90);
    e.quote(
        Quote {
            time: 2,
            bid: 89,
            ask: 90,
        },
        1,
        &c,
    )
    .unwrap();
    assert_eq!(e.signals()[0].state, SignalState::Stop);
    assert!(!e.activate_revision(&revised).unwrap());
    assert_eq!(e.signals()[0].state, SignalState::Stop);
}

#[test]
fn rule_change_preserves_published_levels() {
    let mut c = cfg();
    let mut e = Engine::default();
    for b in [
        bar(1, 280, 300, 270, 280),
        bar(2, 280, 310, 270, 280),
        bar(3, 280, 300, 270, 280),
    ] {
        e.seed(Timeframe::D, b, &c).unwrap();
    }
    for t in 1..=10 {
        e.seed(Timeframe::H4, bar(t, 100, 110, 90, 100), &c)
            .unwrap();
    }
    e.add_plan("D_correction_end", Side::Long, Timeframe::D, poi(1), 10);
    e.closed(Timeframe::H4, bar(11, 125, 140, 120, 130), &c)
        .unwrap();
    e.quote(
        Quote {
            time: 12,
            bid: 130,
            ask: 131,
        },
        1,
        &c,
    )
    .unwrap();
    let before = e.signals();
    assert_eq!(before.len(), 1);
    assert_eq!(before[0].state, SignalState::Active);
    c.parameters.stop_buffer_atr.value = 1.0;
    let changes = e.reconfigure(&c, 1).unwrap();
    assert!(changes.is_empty());
    assert_eq!(
        serde_json::to_value(e.signals()).unwrap(),
        serde_json::to_value(before).unwrap()
    );
}

#[test]
fn inverted_latest_zone_does_not_fall_back_to_older_live_zone() {
    let c = cfg();
    let mut e = Engine::default();
    e.seed(Timeframe::W, bar(1, 100, 120, 90, 110), &c).unwrap();
    let mut snapshot = serde_json::to_value(&e).unwrap();
    let mut recent = Zone::new(Side::Long, 115, 116, 3, ZoneKind::Fvg);
    recent.state = ZoneState::Inverted;
    snapshot["frames"]["W"]["trend"] = serde_json::json!("Long");
    snapshot["frames"]["W"]["zones"] =
        serde_json::json!([{"zone":poi(1),"atr":20.0},{"zone":recent,"atr":20.0}]);
    e = serde_json::from_value(snapshot).unwrap();
    e.closed(Timeframe::D, bar(4, 100, 105, 95, 101), &c)
        .unwrap();
    assert_eq!(e.plan_count(), 0);
}
#[test]
fn weekly_sweep_ignores_fractals_older_than_sixty_bars() {
    let c = cfg();
    let mut e = Engine::default();
    for t in 1..=65 {
        e.seed(Timeframe::W, bar(t, 100, 110, 90, 100), &c).unwrap();
    }
    let mut snapshot = serde_json::to_value(&e).unwrap();
    snapshot["frames"]["W"]["trend"] = serde_json::json!("Long");
    snapshot["frames"]["W"]["fractals"] = serde_json::json!([Fractal {
        price: 120,
        confirmed_at: 2,
        upper: true,
        swept: false
    }]);
    e = serde_json::from_value(snapshot).unwrap();
    e.closed(Timeframe::D, bar(66, 100, 130, 90, 100), &c)
        .unwrap();
    assert!(serde_json::to_value(&e).unwrap()["weekly_fractal_sweep"].is_null());
}

#[test]
fn strategy_format_change_preserves_synthetic_signal_sequence() {
    use sha2::{Digest, Sha256};
    let c = cfg();
    let mut e = Engine::default();
    let mut states = Vec::new();
    for b in [
        bar(1, 280, 300, 270, 280),
        bar(2, 280, 310, 270, 280),
        bar(3, 280, 300, 270, 280),
    ] {
        e.seed(Timeframe::D, b, &c).unwrap();
    }
    for t in 1..=10 {
        e.seed(Timeframe::H4, bar(t, 100, 110, 90, 100), &c)
            .unwrap();
    }
    e.add_plan("D_correction_end", Side::Long, Timeframe::D, poi(1), 10);
    e.closed(Timeframe::H4, bar(11, 125, 140, 120, 130), &c)
        .unwrap();
    for (time, bid, ask) in [
        (12, 130, 131),
        (13, 150, 151),
        (14, 311, 312),
        (14, 311, 312),
    ] {
        e.quote(Quote { time, bid, ask }, 1, &c).unwrap();
        states.push(serde_json::to_value(&e).unwrap());
    }
    assert_eq!(e.signals()[0].state, SignalState::TakeProfit);
    let digest = format!("{:x}", Sha256::digest(serde_json::to_vec(&states).unwrap()));
    assert_eq!(
        digest,
        "01e8661638d7539c136fe03d226a7e3c8418f8bfa06d4356b4147785c5db0e72"
    );
}

fn numeric(c: &forex_bot::model::Strategy, key: &str, n: u64) -> forex_bot::model::Strategy {
    let mut v = serde_json::to_value(c).unwrap();
    v["parameters"][key] = serde_json::json!({"value":n});
    serde_json::from_value(v).unwrap()
}
#[test]
fn explicit_stop_window_changes_extreme_and_requires_enough_bars() {
    let c = cfg();
    let bars = [bar(1, 100, 110, 70, 100), bar(2, 100, 110, 90, 100)];
    let one = numeric(&c, "stop_fallback_h4_bars", 1);
    let two = numeric(&c, "stop_fallback_h4_bars", 2);
    let three = numeric(&c, "stop_fallback_h4_bars", 3);
    assert_eq!(
        select_stop(Side::Long, 95, 100, &[], &bars, 0.0, 1, &one).unwrap(),
        90
    );
    assert_eq!(
        select_stop(Side::Long, 95, 100, &[], &bars, 0.0, 1, &two).unwrap(),
        70
    );
    assert!(select_stop(Side::Long, 95, 100, &[], &bars, 0.0, 1, &three).is_err());
}
#[test]
fn explicit_target_windows_include_boundary_and_exclude_previous_bar() {
    let c = cfg();
    for (tf, key) in [
        (Timeframe::D, "target_fractal_lookback_d_bars"),
        (Timeframe::W, "target_fractal_lookback_w_bars"),
    ] {
        let mut e = Engine::default();
        for t in 1..=4 {
            e.seed(tf, bar(t, 100, 110, 90, 100), &c).unwrap();
        }
        let mut snap = serde_json::to_value(&e).unwrap();
        let k = if tf == Timeframe::D { "D" } else { "W" };
        snap["frames"][k]["fractals"] = serde_json::json!([Fractal {
            price: 150,
            confirmed_at: 3,
            upper: true,
            swept: false
        }]);
        e = serde_json::from_value(snap).unwrap();
        assert_eq!(
            e.target(Side::Long, tf, 100, 90, &numeric(&c, key, 2)),
            Some(150)
        );
        assert_eq!(
            e.target(Side::Long, tf, 100, 90, &numeric(&c, key, 1)),
            None
        );
    }
}
#[test]
fn explicit_weekly_sweep_window_includes_boundary_only() {
    let c = cfg();
    let mut e = Engine::default();
    for t in 1..=4 {
        e.seed(Timeframe::W, bar(t, 100, 110, 90, 100), &c).unwrap();
    }
    let mut snap = serde_json::to_value(&e).unwrap();
    snap["frames"]["W"]["trend"] = serde_json::json!("Long");
    snap["frames"]["W"]["fractals"] = serde_json::json!([Fractal {
        price: 120,
        confirmed_at: 3,
        upper: true,
        swept: false
    }]);
    for (n, hit) in [(1, false), (2, true)] {
        let mut e: Engine = serde_json::from_value(snap.clone()).unwrap();
        e.closed(
            Timeframe::D,
            bar(5, 100, 130, 90, 100),
            &numeric(&c, "weekly_fractal_sweep_lookback_bars", n),
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(&e).unwrap()["weekly_fractal_sweep"].is_number(),
            hit
        );
    }
}
#[test]
fn explicit_confirmation_window_includes_inverted_zone_boundary_only() {
    let c = cfg();
    let mut e = Engine::default();
    for t in 1..=4 {
        e.seed(Timeframe::H4, bar(t, 100, 110, 90, 100), &c)
            .unwrap();
    }
    e.add_plan("W_point_A", Side::Long, Timeframe::W, poi(1), 1);
    let mut snap = serde_json::to_value(&e).unwrap();
    snap["frames"]["H4"]["zones"] =
        serde_json::json!([{"zone":Zone::new(Side::Short,105,110,4,ZoneKind::Fvg),"atr":20.0}]);
    for (n, confirmed) in [(1, false), (2, true)] {
        let mut e: Engine = serde_json::from_value(snap.clone()).unwrap();
        e.closed(
            Timeframe::H4,
            bar(5, 100, 120, 90, 119),
            &numeric(&c, "confirmation_inversion_h4_bars", n),
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(&e).unwrap()["plans"][0]["state"] != "WaitConfirm",
            confirmed
        );
    }
}

fn seeded_correction(c: &forex_bot::model::Strategy) -> serde_json::Value {
    let mut e = Engine::default();
    for tf in [Timeframe::W, Timeframe::D] {
        for b in [
            bar(1, 100, 110, 90, 105),
            bar(2, 105, 125, 100, 120),
            bar(3, 125, 135, 120, 130),
        ] {
            e.seed(tf, b, c).unwrap();
        }
    }
    e.closed(Timeframe::D, bar(4, 130, 135, 90, 95), c).unwrap();
    serde_json::to_value(&e).unwrap()
}

#[test]
fn disabled_rule_does_not_create_plans_and_other_rules_are_unaffected() {
    let rules = |s: &serde_json::Value| -> Vec<String> {
        s["plans"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["rule"].as_str().unwrap().to_owned())
            .collect()
    };
    let enabled = rules(&seeded_correction(&cfg()));
    assert!(enabled.contains(&"D_correction_start".to_owned()));
    let mut c = cfg();
    c.policies.plans.disabled_rules =
        serde_json::from_value(serde_json::json!(["D_correction_start"])).unwrap();
    let disabled = rules(&seeded_correction(&c));
    assert!(!disabled.contains(&"D_correction_start".to_owned()));
    let mut expected = enabled.clone();
    expected.retain(|r| r != "D_correction_start");
    assert_eq!(disabled, expected);
    let mut c = cfg();
    c.policies.plans.disabled_rules =
        serde_json::from_value(serde_json::json!(["W_point_A"])).unwrap();
    assert_eq!(
        rules(&seeded_correction(&c)),
        enabled
            .into_iter()
            .filter(|r| r != "W_point_A")
            .collect::<Vec<_>>()
    );
}

#[test]
fn disabled_rules_accepts_only_unique_plan_rules_and_defaults_to_empty() {
    let base: serde_json::Value =
        serde_json::from_slice(&std::fs::read("../strategy/fvg-bot-rules.json").unwrap()).unwrap();
    let load = |v: &serde_json::Value| {
        let p = std::env::temp_dir().join(format!("disabled-rules-{}.json", std::process::id()));
        std::fs::write(&p, v.to_string()).unwrap();
        let r = forex_bot::load(&p);
        std::fs::remove_file(&p).unwrap();
        r
    };
    let mut v = base.clone();
    v["policies"]["plans"]
        .as_object_mut()
        .unwrap()
        .remove("disabled_rules");
    assert!(load(&v).unwrap().policies.plans.disabled_rules.is_empty());
    for bad in [
        serde_json::json!(["W_fractal_swept"]),
        serde_json::json!(["D_point_A"]),
        serde_json::json!(["W_point_A", "W_point_A"]),
        serde_json::json!(["nonexistent"]),
    ] {
        let mut v = base.clone();
        v["policies"]["plans"]["disabled_rules"] = bad.clone();
        assert!(load(&v).is_err(), "accepted {bad}");
    }
    let mut v = base;
    v["policies"]["plans"]["disabled_rules"] = serde_json::json!([
        "W_point_A",
        "D_continuation_from_weekly_zone",
        "D_correction_end"
    ]);
    assert_eq!(load(&v).unwrap().policies.plans.disabled_rules.len(), 3);
}
