package com.breezone.app.data

import kotlinx.serialization.Serializable

/** What a device on the solar surplus does now. */
@Serializable
data class SurplusStatus(val state: String = "", val text: String = "")

/** A device an automation put on the solar surplus (`GET /api/surplus`). */
@Serializable
data class SurplusLoad(
    val deviceId: Long,
    val name: String = "",
    /** "charger" | "relay" | "climate" */
    val kind: String = "relay",
    val automationId: Long? = null,
    val automation: String? = null,
    val priority: Int? = null,
    val power: Double? = null,
    val minOnMinutes: Int = 0,
    /** A zone: "heat" or "cool", to [setpoint]. */
    val mode: String? = null,
    val setpoint: Double? = null,
    val status: SurplusStatus? = null,
)

/** The devices on the solar surplus, in the order it is shared. */
@Serializable
data class SurplusView(val meter: Boolean? = null, val export: Double? = null, val loads: List<SurplusLoad> = emptyList())
