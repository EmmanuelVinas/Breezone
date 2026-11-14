// Generated from server/contracts/traits by `BREEZONE_WRITE_GENERATED=1 cargo test --test generated` (in server/): do not edit.

package com.breezone.app.data

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
enum class ClimateZoneMode {
    @SerialName("stop") Stop,
    @SerialName("cooling") Cooling,
    @SerialName("heating") Heating,
    @SerialName("fan") Fan,
    @SerialName("dry") Dry,
    @SerialName("aux_heating") AuxHeating,
    @SerialName("auto") Auto,
}

@Serializable
enum class ClimateZoneAirQuality {
    @SerialName("good") Good,
    @SerialName("regular") Regular,
    @SerialName("bad") Bad,
}

/** Air conditioning (`climate.zone@1`): state. */
@Serializable
data class ClimateZoneState(
    val on: Boolean = false,
    val roomTemp: Double? = null,
    val humidity: Long? = null,
    val setpoint: Double? = null,
    val coolSetpoint: Double? = null,
    val heatSetpoint: Double? = null,
    val mode: ClimateZoneMode? = null,
    val speed: Long? = null,
    val sleep: Long? = null,
    val demand: Boolean = false,
    val airQuality: ClimateZoneAirQuality? = null,
)

/** Air conditioning (`climate.zone@1`): action `set`. */
@Serializable
data class ClimateZoneSet(
    val on: Boolean? = null,
    val setpoint: Double? = null,
    val coolSetpoint: Double? = null,
    val heatSetpoint: Double? = null,
    val mode: ClimateZoneMode? = null,
    val speed: Long? = null,
    val sleep: Long? = null,
)

/** Energy meter (`energy.meter@1`): state. */
@Serializable
data class EnergyMeterState(
    val energyIn: Double = 0.0,
    val energyOut: Double? = null,
)

/** Power (`energy.power@1`): state. */
@Serializable
data class EnergyPowerState(
    val power: Double = 0.0,
    val powerA: Double? = null,
    val powerB: Double? = null,
    val powerC: Double? = null,
    val voltage: Double? = null,
    val current: Double? = null,
)

/** Relay (`switch.relay@1`): state. */
@Serializable
data class SwitchRelayState(
    val on: Boolean = false,
)

/** Relay (`switch.relay@1`): action `set`. */
@Serializable
data class SwitchRelaySet(
    val on: Boolean? = null,
)

/** Traits described by this version (`trait@version`). */
val TRAITS = listOf("climate.zone@1", "energy.meter@1", "energy.power@1", "switch.relay@1")
