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
