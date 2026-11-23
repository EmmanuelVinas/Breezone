package com.breezone.app.data

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/** What a slot of a charging profile does (`ChargeMode`). */
@Serializable
enum class ChargeMode {
    @SerialName("off") Off,
    @SerialName("max") Max,
    @SerialName("solar") Solar,
}

@Serializable
data class ChargeProfileRef(val id: Long, val name: String)

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
data class ChargeNext(val start: Long, val end: Long, val mode: ChargeMode, val kw: Double? = null, val color: DayColor? = null)

@Serializable
data class ChargePeriod(val start: Long, val end: Long, val mode: ChargeMode, val kw: Double? = null, val color: DayColor? = null)

@Serializable
data class ChargeDay(val day: String, val color: DayColor? = null, val periods: List<ChargePeriod> = emptyList())

/** What a charger follows and does (`GET /api/devices/{id}/charging`). */
@Serializable
data class ChargingView(
    val profileId: Long? = null,
    val profiles: List<ChargeProfileRef> = emptyList(),
    val now: ChargeNow? = null,
    val status: ChargeStatus? = null,
    val next: ChargeNext? = null,
    val days: List<ChargeDay> = emptyList(),
    val canControl: Boolean = false,
)

@Serializable
data class ChargeProfileInput(val profileId: Long?)

/** "Charge now": up to `kw` (the charger's maximum without), until a time or `kwh` more. */
@Serializable
data class ChargeNowInput(val kw: Double? = null, val until: Long? = null, val kwh: Double? = null)

/** A slot of a kind of day, until the next one (`ChargeSlot`). */
@Serializable
data class ChargeSlot(
    /** "HH:MM", local time; the first at "00:00". */
    val start: String,
    val mode: ChargeMode,
    val kw: Double? = null,
    val minAmps: Long? = null,
    val maxAmps: Long? = null,
)

@Serializable
data class ChargeDayKind(val id: String, val name: String, val color: String? = null, val slots: List<ChargeSlot> = emptyList())

/** The kind of day of each Tempo colour, when the calendar follows them. */
@Serializable
data class ChargeColors(val blue: String? = null, val white: String? = null, val red: String? = null) {
    val any get() = blue != null || white != null || red != null
}

/** Which kind of day a day follows (`TariffCalendar`). */
@Serializable
data class ChargeCalendar(
    /** "HH:MM": when a day starts ("06:00" for Tempo). */
    val dayStart: String = "00:00",
    /** Monday first. */
    val weekdays: List<String> = emptyList(),
    val colors: ChargeColors = ChargeColors(),
)

@Serializable
data class ChargeUsedBy(val deviceId: Long, val name: String)

/** A charging profile (`ChargeProfileView`). */
@Serializable
data class ChargeProfileView(
    val id: Long,
    val name: String,
    val profiles: List<ChargeDayKind> = emptyList(),
    val calendar: ChargeCalendar = ChargeCalendar(),
    val usedBy: List<ChargeUsedBy> = emptyList(),
    val automations: List<String> = emptyList(),
)

/** A charging profile as it is written. */
@Serializable
data class ChargeProfileWrite(val name: String, val profiles: List<ChargeDayKind>, val calendar: ChargeCalendar)
