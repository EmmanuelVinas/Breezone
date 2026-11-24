package com.breezone.app.data

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/** What a charger was told by the automations (`ChargeMode`). */
@Serializable
enum class ChargeMode {
    @SerialName("profile") Profile,
    @SerialName("off") Off,
    @SerialName("solar") Solar,
    @SerialName("free") Free,
}

/** What an automation tells a charger, on a frieze (`ChargeEffectMode`). */
@Serializable
enum class ChargeEffectMode {
    @SerialName("profile") Profile,
    @SerialName("off") Off,
    @SerialName("solar") Solar,
    @SerialName("free") Free,
    @SerialName("now") Now,
}

@Serializable
data class ChargeEffect(val mode: ChargeEffectMode, val profileId: Long? = null, val kw: Double? = null)

/** An automation (or a charger) by its id and name. */
@Serializable
data class ChargeRuleRef(val id: Long, val name: String)

/** A profile, for the friezes. */
@Serializable
data class ChargeProfileRef(val id: Long, val name: String, val kw: Double? = null, val color: String? = null)

/** A charge asked now (`ChargeNow`). */
@Serializable
data class ChargeNow(
    val amps: Long,
    val kw: Double? = null,
    val until: Long? = null,
    val kwh: Double? = null,
    val leftKwh: Double? = null,
)

@Serializable
data class ChargeStatus(val state: String, val reason: String = "")

@Serializable
data class ChargeNext(val start: Long, val end: Long, val effect: ChargeEffect, val kw: Double? = null, val automation: ChargeRuleRef? = null)

@Serializable
data class ChargeSegment(val start: Long, val end: Long, val effect: ChargeEffect? = null, val automationId: Long? = null)

@Serializable
data class ChargeDay(val day: String, val color: DayColor? = null, val segments: List<ChargeSegment> = emptyList())

/** What a charger follows and does (`GET /api/devices/{id}/charging`). */
@Serializable
data class ChargingView(
    val mode: ChargeMode = ChargeMode.Free,
    val profileId: Long? = null,
    val automation: ChargeRuleRef? = null,
    val profiles: List<ChargeProfileRef> = emptyList(),
    val ownSchedule: Boolean = false,
    val now: ChargeNow? = null,
    val status: ChargeStatus? = null,
    val next: ChargeNext? = null,
    val days: List<ChargeDay> = emptyList(),
    val automations: List<ChargeRuleRef> = emptyList(),
    val canControl: Boolean = false,
)

/** "Charge now": up to `kw` (the charger's maximum without), until a time or `kwh` more. */
@Serializable
data class ChargeNowInput(val kw: Double? = null, val until: Long? = null, val kwh: Double? = null)

/** A charging profile: a power (`ChargeProfileView`). */
@Serializable
data class ChargeProfileView(
    val id: Long,
    val name: String,
    /** Up to this power (null: the charger's maximum). */
    val kw: Double? = null,
    val color: String? = null,
    val automations: List<ChargeRuleRef> = emptyList(),
)

/** A charging profile as it is written. */
@Serializable
data class ChargeProfileWrite(val name: String, val kw: Double? = null, val color: String? = null)

/** A stretch of a day of a frieze, in minutes from midnight. */
@Serializable
data class ChargeDaySegment(val start: Int, val end: Int, val effect: ChargeEffect? = null)

/** A frieze of an automation for a charger: a day of a kind of the tariff, or any day. */
@Serializable
data class ChargeBar(
    val deviceId: Long,
    val kind: String? = null,
    val label: String = "",
    val color: String? = null,
    val others: Boolean = false,
    val segments: List<ChargeDaySegment> = emptyList(),
)

/** An automation in words, in the language of the request. */
@Serializable
data class ChargeRuleSummary(
    val `when`: String = "",
    val conditions: List<String> = emptyList(),
    val joined: String = "",
    val actions: List<String> = emptyList(),
    @SerialName("else") val otherwise: List<String> = emptyList(),
    val over: List<String> = emptyList(),
)

/** An automation that drives a charger, with its friezes (`GET /api/charging/automations`). */
@Serializable
data class ChargeRulePlan(
    val id: Long,
    val name: String,
    val enabled: Boolean = true,
    val summary: ChargeRuleSummary = ChargeRuleSummary(),
    val devices: List<ChargeRuleRef> = emptyList(),
    val paused: Boolean = false,
    val placed: Boolean = false,
    val assumed: Boolean = false,
    val bars: List<ChargeBar> = emptyList(),
)
