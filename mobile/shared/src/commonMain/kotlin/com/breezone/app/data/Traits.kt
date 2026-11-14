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

@Serializable
enum class EvChargerStatus {
    @SerialName("available") Available,
    @SerialName("plugged") Plugged,
    @SerialName("charging") Charging,
    @SerialName("fault") Fault,
}

/** EV charger (`ev.charger@1`): state. */
@Serializable
data class EvChargerState(
    val status: EvChargerStatus? = null,
    val vehicle: Boolean? = null,
    val paused: Boolean = false,
    val locked: Boolean = false,
    val sessionEnergy: Double = 0.0,
    val sessionSecs: Long = 0,
    val limit: Long? = null,
    val limitMin: Long? = null,
    val limitMax: Long? = null,
    val autoLimit: Boolean? = null,
    val energyGoal: Double? = null,
)

/** EV charger (`ev.charger@1`): action `set_limit`. */
@Serializable
data class EvChargerSetLimit(
    val limit: Long? = null,
)

/** EV charger (`ev.charger@1`): action `start`. */
@Serializable
data class EvChargerStart(
    val limit: Long? = null,
    val energy: Double? = null,
)

/** EV charger (`ev.charger@1`): action `stop`, without parameters. */
@Serializable
class EvChargerStop

/** EV charger (`ev.charger@1`): action `lock`. */
@Serializable
data class EvChargerLock(
    val locked: Boolean? = null,
)

/** EV charger (`ev.charger@1`): action `set_auto_limit`. */
@Serializable
data class EvChargerSetAutoLimit(
    val on: Boolean? = null,
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
val TRAITS = listOf("climate.zone@1", "energy.meter@1", "ev.charger@1", "energy.power@1", "switch.relay@1")
