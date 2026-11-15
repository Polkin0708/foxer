use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Parameter<T> {
    pub value: T,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Item {
    pub symbol: String,
    pub price_tick: i64,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CandlesWeeklyBoundary {
    pub time: String,
    pub weekday: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Candles {
    pub daily_boundary: String,
    pub h4_close_hours: Vec<u64>,
    pub only_closed_bars: bool,
    pub price_basis: String,
    pub timezone: String,
    pub weekly_boundary: CandlesWeeklyBoundary,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextBarsAnalyzed {
    #[serde(rename = "D")]
    pub d: u64,
    #[serde(rename = "H4")]
    pub h4: u64,
    #[serde(rename = "W")]
    pub w: u64,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextMinimumBars {
    #[serde(rename = "D")]
    pub d: u64,
    #[serde(rename = "H4")]
    pub h4: u64,
    #[serde(rename = "W")]
    pub w: u64,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    pub bars_analyzed: ContextBarsAnalyzed,
    pub minimum_bars: ContextMinimumBars,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Parameters {
    pub atr_period: Parameter<u64>,
    pub atr_reference_zone: Parameter<String>,
    pub correction_entry_zones_from: Parameter<String>,
    pub ifvg_entry: Parameter<bool>,
    pub ifvg_limit_range: Parameter<String>,
    pub ifvg_max_zone_age: Parameter<String>,
    pub inv_margin_atr: Parameter<f64>,
    pub min_fvg_atr: Parameter<f64>,
    pub plan_days: Parameter<u64>,
    pub plan_days_statuses: Parameter<Vec<String>>,
    pub rr_max_correction: Parameter<f64>,
    pub rr_max_trend: Parameter<f64>,
    pub rr_min: Parameter<f64>,
    pub stop_buffer_atr: Parameter<f64>,
    pub trend_fvg_atr: Parameter<f64>,
    pub stop_fallback_h4_bars: Parameter<u64>,
    pub confirmation_inversion_h4_bars: Parameter<u64>,
    pub weekly_fractal_sweep_lookback_bars: Parameter<u64>,
    pub target_fractal_lookback_d_bars: Parameter<u64>,
    pub target_fractal_lookback_w_bars: Parameter<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum PoliciesAtrMethod {
    #[serde(rename = "mean_high_low")]
    MeanHighLow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum PoliciesAtrStopReference {
    #[serde(rename = "last_closed_h4")]
    LastClosedH4,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PoliciesAtr {
    pub method: PoliciesAtrMethod,
    pub stop_reference: PoliciesAtrStopReference,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum PoliciesZonesFvg {
    #[serde(rename = "three_candle_gap")]
    ThreeCandleGap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum PoliciesZonesGap {
    #[serde(rename = "previous_close_to_next_open")]
    PreviousCloseToNextOpen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum PoliciesZonesLifecycle {
    #[serde(rename = "reaction_before_inversion")]
    ReactionBeforeInversion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum PoliciesZonesTrend {
    #[serde(rename = "weighted_live_zones")]
    WeightedLiveZones,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum PoliciesZonesSelection {
    #[serde(rename = "latest_formation_bar")]
    LatestFormationBar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum PoliciesZonesTiebreak {
    #[serde(rename = "nearest_to_price")]
    NearestToPrice,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PoliciesZones {
    pub fvg: PoliciesZonesFvg,
    pub gap: PoliciesZonesGap,
    pub lifecycle: PoliciesZonesLifecycle,
    pub trend: PoliciesZonesTrend,
    pub selection: PoliciesZonesSelection,
    pub tiebreak: PoliciesZonesTiebreak,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum PoliciesFractalsFormation {
    #[serde(rename = "strict_three_bar_extremum")]
    StrictThreeBarExtremum,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum PoliciesFractalsSweep {
    #[serde(rename = "strict_extremum_break")]
    StrictExtremumBreak,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PoliciesFractals {
    pub formation: PoliciesFractalsFormation,
    pub sweep: PoliciesFractalsSweep,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum PoliciesPlansWeeklyRulesItem {
    #[serde(rename = "W_trend_new")]
    WTrendNew,
    #[serde(rename = "W_trend_end")]
    WTrendEnd,
    #[serde(rename = "W_trend_flip")]
    WTrendFlip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum PoliciesPlansDailyRulesItem {
    #[serde(rename = "D_first_zone_ignored")]
    DFirstZoneIgnored,
    #[serde(rename = "D_correction_start")]
    DCorrectionStart,
    #[serde(rename = "D_correction_end")]
    DCorrectionEnd,
    #[serde(rename = "D_continuation_from_weekly_zone")]
    DContinuationFromWeeklyZone,
    #[serde(rename = "D_correction_after_weekly_fractal")]
    DCorrectionAfterWeeklyFractal,
    #[serde(rename = "D_point_A")]
    DPointA,
    #[serde(rename = "D_trend_weakness")]
    DTrendWeakness,
    #[serde(rename = "W_point_A")]
    WPointA,
    #[serde(rename = "W_fractal_swept")]
    WFractalSwept,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum PoliciesPlansRolesW {
    #[serde(rename = "direction")]
    Direction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum PoliciesPlansRolesD {
    #[serde(rename = "plans")]
    Plans,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum PoliciesPlansRolesH4 {
    #[serde(rename = "confirmation_entry_tracking")]
    ConfirmationEntryTracking,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PoliciesPlansRoles {
    #[serde(rename = "W")]
    pub w: PoliciesPlansRolesW,
    #[serde(rename = "D")]
    pub d: PoliciesPlansRolesD,
    #[serde(rename = "H4")]
    pub h4: PoliciesPlansRolesH4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum PoliciesPlansDailyPoi {
    #[serde(rename = "triggering_daily_zone")]
    TriggeringDailyZone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum PoliciesPlansWeeklyPoi {
    #[serde(rename = "latest_live_weekly_zone")]
    LatestLiveWeeklyZone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum PoliciesPlansConfirmationItem {
    #[serde(rename = "new_fvg_in_plan_direction")]
    NewFvgInPlanDirection,
    #[serde(rename = "opposite_fvg_inversion")]
    OppositeFvgInversion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum PoliciesPlansEntryItem {
    #[serde(rename = "new_fvg_after_plan_start")]
    NewFvgAfterPlanStart,
    #[serde(rename = "opposite_fvg_inversion")]
    OppositeFvgInversion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum PoliciesPlansEntryPriority {
    #[serde(rename = "latest_new_fvg")]
    LatestNewFvg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum PoliciesPlansStop {
    #[serde(rename = "latest_protective_fractal_or_window_extreme")]
    LatestProtectiveFractalOrWindowExtreme,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PoliciesPlans {
    pub weekly_rules: Vec<PoliciesPlansWeeklyRulesItem>,
    pub daily_rules: Vec<PoliciesPlansDailyRulesItem>,
    pub daily_requires_weekly_trend: bool,
    pub roles: PoliciesPlansRoles,
    pub daily_poi: PoliciesPlansDailyPoi,
    pub weekly_poi: PoliciesPlansWeeklyPoi,
    pub confirmation: Vec<PoliciesPlansConfirmationItem>,
    pub entry: Vec<PoliciesPlansEntryItem>,
    pub entry_priority: PoliciesPlansEntryPriority,
    pub stop: PoliciesPlansStop,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disabled_rules: Vec<PoliciesPlansDailyRulesItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum PoliciesExecutionAboveMaximum {
    #[serde(rename = "cap_take_profit_at_rr_max")]
    CapTakeProfitAtRrMax,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum PoliciesExecutionBelowMinimum {
    #[serde(rename = "limit_entry_at_rr_min_keep_stop_and_target")]
    LimitEntryAtRrMinKeepStopAndTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum PoliciesExecutionEventOrder {
    #[serde(rename = "observed_quotes_not_assumed_candle_path")]
    ObservedQuotesNotAssumedCandlePath,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum PoliciesExecutionMarketQuote {
    #[serde(rename = "first_available_after_confirmation")]
    FirstAvailableAfterConfirmation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum PoliciesExecutionMarketTiming {
    #[serde(rename = "immediately_after_confirming_h4_close")]
    ImmediatelyAfterConfirmingH4Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum PoliciesExecutionPendingCancelOnItem {
    #[serde(rename = "target_reached_before_entry")]
    TargetReachedBeforeEntry,
    #[serde(rename = "stop_reached_before_entry")]
    StopReachedBeforeEntry,
    #[serde(rename = "inverted_zone_broken_before_entry")]
    InvertedZoneBrokenBeforeEntry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum PoliciesExecutionLimitEntry {
    #[serde(rename = "solve_rr_min_keep_stop_target")]
    SolveRrMinKeepStopTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum PoliciesExecutionPendingZoneBreak {
    #[serde(rename = "h4_close_beyond_inverted_zone_with_atr_margin")]
    H4CloseBeyondInvertedZoneWithAtrMargin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum PoliciesExecutionEntryMode {
    #[serde(rename = "auto")]
    Auto,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PoliciesExecution {
    pub above_maximum: PoliciesExecutionAboveMaximum,
    pub below_minimum: PoliciesExecutionBelowMinimum,
    pub event_order: PoliciesExecutionEventOrder,
    pub market_quote: PoliciesExecutionMarketQuote,
    pub market_timing: PoliciesExecutionMarketTiming,
    pub pending_cancel_on: Vec<PoliciesExecutionPendingCancelOnItem>,
    pub limit_entry: PoliciesExecutionLimitEntry,
    pub pending_zone_break: PoliciesExecutionPendingZoneBreak,
    pub entry_mode: PoliciesExecutionEntryMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum PoliciesTargetsFvgBoundary {
    #[serde(rename = "near")]
    Near,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum PoliciesTargetsSelection {
    #[serde(rename = "nearest_eligible_in_trade_direction")]
    NearestEligibleInTradeDirection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum PoliciesTargetsTimeframeByPoiD {
    #[serde(rename = "D")]
    D,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum PoliciesTargetsTimeframeByPoiW {
    #[serde(rename = "W")]
    W,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PoliciesTargetsTimeframeByPoi {
    #[serde(rename = "D")]
    pub d: PoliciesTargetsTimeframeByPoiD,
    #[serde(rename = "W")]
    pub w: PoliciesTargetsTimeframeByPoiW,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum PoliciesTargetsTypesItem {
    #[serde(rename = "unswept_fractal")]
    UnsweptFractal,
    #[serde(rename = "fvg")]
    Fvg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum PoliciesTargetsWeeklyPoiDailyAlternativeMinimumRr {
    #[serde(rename = "rr_min")]
    RrMin,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PoliciesTargetsWeeklyPoiDailyAlternative {
    pub enabled: bool,
    pub must_be_closer: bool,
    pub minimum_rr: PoliciesTargetsWeeklyPoiDailyAlternativeMinimumRr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum PoliciesTargetsMode {
    #[serde(rename = "structural_only")]
    StructuralOnly,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PoliciesTargets {
    pub fvg_boundary: PoliciesTargetsFvgBoundary,
    pub selection: PoliciesTargetsSelection,
    pub timeframe_by_poi: PoliciesTargetsTimeframeByPoi,
    pub types: Vec<PoliciesTargetsTypesItem>,
    pub weekly_poi_daily_alternative: PoliciesTargetsWeeklyPoiDailyAlternative,
    pub mode: PoliciesTargetsMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum PoliciesTradeWindowMode {
    #[serde(rename = "all_available_market_hours")]
    AllAvailableMarketHours,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PoliciesTradeWindow {
    pub mode: PoliciesTradeWindowMode,
    pub session_filter: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum PoliciesPositionsParallelTrades {
    #[serde(rename = "independent_plans")]
    IndependentPlans,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum PoliciesPositionsOneEntryPerPlan {
    #[serde(rename = "until_signal_closed")]
    UntilSignalClosed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum PoliciesPositionsSimultaneousSetups {
    #[serde(rename = "independent_plans")]
    IndependentPlans,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum PoliciesPositionsSignalIdentity {
    #[serde(rename = "idempotent_setup")]
    IdempotentSetup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum PoliciesPositionsRuleUpdate {
    #[serde(rename = "recalculate_waiting_preserve_published_levels")]
    RecalculateWaitingPreservePublishedLevels,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PoliciesPositions {
    pub parallel_trades: PoliciesPositionsParallelTrades,
    pub one_entry_per_plan: PoliciesPositionsOneEntryPerPlan,
    pub simultaneous_setups: PoliciesPositionsSimultaneousSetups,
    pub signal_identity: PoliciesPositionsSignalIdentity,
    pub rule_update: PoliciesPositionsRuleUpdate,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PoliciesModules {
    pub range_trading: bool,
    pub daily_point_a: bool,
    pub weak_correction: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum PoliciesRiskCountedStatusesItem {
    #[serde(rename = "active")]
    Active,
    #[serde(rename = "setup")]
    Setup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum PoliciesRiskQueueOrderItem {
    #[serde(rename = "signal_time")]
    SignalTime,
    #[serde(rename = "rr_desc")]
    RrDesc,
    #[serde(rename = "instrument_order")]
    InstrumentOrder,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PoliciesRisk {
    pub max_open_risk_r: Option<f64>,
    pub per_trade_r: f64,
    pub counted_statuses: Vec<PoliciesRiskCountedStatusesItem>,
    pub queue_order: Vec<PoliciesRiskQueueOrderItem>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Policies {
    pub atr: PoliciesAtr,
    pub zones: PoliciesZones,
    pub fractals: PoliciesFractals,
    pub plans: PoliciesPlans,
    pub execution: PoliciesExecution,
    pub targets: PoliciesTargets,
    pub trade_window: PoliciesTradeWindow,
    pub positions: PoliciesPositions,
    pub modules: PoliciesModules,
    pub risk: PoliciesRisk,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Messages {
    #[serde(rename = "W_trend_new")]
    pub w_trend_new: String,
    #[serde(rename = "W_trend_end")]
    pub w_trend_end: String,
    #[serde(rename = "W_trend_flip")]
    pub w_trend_flip: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StrategyFields {
    pub schema_version: u64,
    pub instrument_price_tick_scale: u64,
    pub instruments: Vec<Item>,
    pub candles: Candles,
    pub context: Context,
    pub parameters: Parameters,
    pub policies: Policies,
    pub messages: Messages,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(try_from = "crate::document::StrictValue")]
pub struct Strategy {
    pub schema_version: u64,
    pub instrument_price_tick_scale: u64,
    pub instruments: Vec<Item>,
    pub candles: Candles,
    pub context: Context,
    pub parameters: Parameters,
    pub policies: Policies,
    pub messages: Messages,
}

impl TryFrom<crate::document::StrictValue> for Strategy {
    type Error = String;
    fn try_from(v: crate::document::StrictValue) -> Result<Self, String> {
        if v.0
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            != Some(2)
        {
            return Err("unsupported: strategy schema_version; expected 2".into());
        }
        let fields: StrategyFields =
            serde_json::from_value(crate::document::without_comments(&v.0)?)
                .map_err(|e| format!("structure: {e}"))?;
        Ok(Self {
            schema_version: fields.schema_version,
            instrument_price_tick_scale: fields.instrument_price_tick_scale,
            instruments: fields.instruments,
            candles: fields.candles,
            context: fields.context,
            parameters: fields.parameters,
            policies: fields.policies,
            messages: fields.messages,
        })
    }
}
