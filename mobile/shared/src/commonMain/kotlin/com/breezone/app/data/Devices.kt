package com.breezone.app.data

import kotlinx.serialization.KSerializer
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject

/** A device of a server (`/api/devices`): an Airzone zone or a device of a module. */
@Serializable
data class DeviceView(
    val id: Long,
    val kind: String = "",
    val traits: List<String> = emptyList(),
    val source: String = "",
    val sourceId: Long? = null,
    val installationId: Long? = null,
    val reference: JsonElement = JsonNull,
    val name: String = "",
    val roomId: Long? = null,
    /** For an energy measure: "grid" | "solar" | "battery" | "consumption". */
    val role: String? = null,
    val position: Long = 0,
    val available: Boolean = true,
    val state: JsonObject? = null,
    /** This user may drive it (absent before server 0.11: the server then decides). */
    val canControl: Boolean? = null,
) {
    fun has(traitId: String) = traitId in traits

    /** Its controls are enabled: it answers, and this user may drive it. */
    val usable get() = available && canControl != false

    /** The state as a trait describes it (its defaults when unknown). */
    fun <T> stateAs(serializer: KSerializer<T>): T? =
        state?.let { runCatching { FzJson.decodeFromJsonElement(serializer, it) }.getOrNull() }
}

@Serializable
data class Room(
    val id: Long,
    val name: String,
    val position: Long = 0,
)

/** A scene of a module's own app (Hue). */
@Serializable
data class SceneView(
    val sourceId: Long,
    val id: String,
    val name: String,
    val room: String? = null,
    val deviceIds: List<Long> = emptyList(),
)

@Serializable
data class DeviceActionInput(
    val trait: String,
    val action: String,
    val params: JsonObject,
)

/** What a server's modules provide (lights, shutters, ventilation…), read on demand. */
data class ServerDevices(
    val devices: List<DeviceView> = emptyList(),
    val rooms: List<Room> = emptyList(),
    val scenes: List<SceneView> = emptyList(),
    val loaded: Boolean = false,
) {
    /** The devices of modules (not the Airzone zones, shown by the climate screens). */
    val modules: List<DeviceView> get() = devices.filter { it.sourceId != null }
    fun withTrait(traitId: String) = modules.filter { it.has(traitId) }
}

object TraitIds {
    const val LIGHT = "light@1"
    const val RELAY = "switch.relay@1"
    const val COVER = "cover.position@1"
    const val VENTILATION_UNIT = "ventilation.unit@1"
    const val VENTILATION_ROOM = "ventilation.room@1"
    const val CHARGER = "ev.charger@1"
    const val INVERTER = "solar.inverter@1"
    const val PANEL = "solar.panel@1"
    const val POWER = "energy.power@1"
    const val FORECAST = "solar.forecast@1"
}

/**
 * The modules are a server's own: they go with its installations, not with one reached through
 * another Breezone server (unless the server has no installation of its own).
 */
fun ServerState.modulesHere(installation: Installation?): Boolean =
    installation?.access != "node" || installations.none { it.access == "local" }

enum class ChargerShown { Available, Plugged, Charging, Paused, Fault }

/** Paused only matters with a car plugged in, and a paused charger may still say "charging" at 0 W. */
val EvChargerState.shown: ChargerShown
    get() {
        val plugged = vehicle == true || status == EvChargerStatus.Plugged || status == EvChargerStatus.Charging
        return when {
            status == EvChargerStatus.Fault -> ChargerShown.Fault
            paused && plugged -> ChargerShown.Paused
            status == EvChargerStatus.Charging -> ChargerShown.Charging
            status == EvChargerStatus.Plugged -> ChargerShown.Plugged
            status == null -> ChargerShown.Fault
            else -> ChargerShown.Available
        }
    }

/** The curves a device forecasts (`/api/devices/{id}/forecast`): `[ts, value]` points by name. */
@Serializable
data class DeviceForecast(val series: Map<String, List<List<Double>>> = emptyMap())

/** The history of a device (`/api/devices/{id}/history`): `[ts, value]` points by field. */
@Serializable
data class DeviceHistory(
    val bucket: Long = 0,
    val series: Map<String, List<List<Double>>> = emptyMap(),
    /** The oldest point of the device (seconds): the periods offered stop there. */
    val oldest: Long? = null,
)

/** Which roles the home has a measure of (`/api/energy/live`). */
@Serializable
data class EnergyRoles(
    val grid: Boolean = false,
    val solar: Boolean = false,
    val battery: Boolean = false,
    val consumption: Boolean = false,
) {
    val any get() = grid || solar || battery || consumption
}

/** What a consumption measure measures, with its power now (W) or energy (kWh). */
@Serializable
data class EnergyConsumer(
    val name: String,
    val deviceId: Long? = null,
    val roomId: Long? = null,
    val power: Double? = null,
    val energy: Double? = null,
)

/** The home now (W): grid drawn (negative: returned), solar, battery charging (negative: discharging), home. */
@Serializable
data class EnergyLive(
    val roles: EnergyRoles = EnergyRoles(),
    val grid: Double? = null,
    val solar: Double? = null,
    val battery: Double? = null,
    val home: Double? = null,
    val consumers: List<EnergyConsumer> = emptyList(),
    val other: Double? = null,
)

@Serializable
data class EnergyPoint(
    val ts: Long,
    val grid: Double? = null,
    val solar: Double? = null,
    val battery: Double? = null,
    val home: Double? = null,
)

/** Energy of the home over a day (kWh); null for what it has no measure of. */
@Serializable
data class DayEnergy(
    val day: String = "",
    val imported: Double? = null,
    val exported: Double? = null,
    val production: Double? = null,
    val charged: Double? = null,
    val discharged: Double? = null,
    val consumption: Double? = null,
    val selfConsumed: Double? = null,
    val consumers: List<EnergyConsumer> = emptyList(),
    val other: Double? = null,
)

@Serializable
data class EnergyDay(
    val date: String = "",
    val bucket: Long = 900,
    val points: List<EnergyPoint> = emptyList(),
    val totals: DayEnergy = DayEnergy(),
)

@Serializable
data class EnergyRange(val firstDay: String? = null)

/** The oldest point of a history (seconds): the periods offered stop there. */
@Serializable
data class HistoryRange(val oldest: Long? = null)

/** Periods of a curve over time, in hours; a longer one only when the history goes back further. */
object Periods {
    const val DAY = 24
    const val WEEK = 24 * 7
    const val MONTH = 24 * 30
    const val YEAR = 24 * 365
    const val FIVE_YEARS = 24 * 365 * 5
    val ALL = listOf(DAY, WEEK, MONTH, YEAR, FIVE_YEARS)

    fun available(oldest: Long?, now: Long, all: List<Int> = ALL): List<Int> {
        val age = if (oldest == null) 0L else now - oldest
        return all.filterIndexed { i, _ -> i == 0 || age > all[i - 1] * 3600L }
    }

    /** Seconds per point: about one every 15 min on a day, one a month over five years. */
    fun bucket(hours: Int): Int = when {
        hours <= DAY -> 900
        hours <= WEEK -> 3600
        hours <= MONTH -> 4 * 3600
        hours <= YEAR -> 86_400
        else -> 30 * 86_400
    }
}

/** The colour of a Tempo day. */
@Serializable
enum class DayColor {
    @SerialName("blue") Blue,
    @SerialName("white") White,
    @SerialName("red") Red,
}

/** The price of the kWh now: its slot (peak, off-peak…) and its profile (blue day…). */
@Serializable
data class PriceAt(
    val price: Double,
    val slot: String = "",
    val profileName: String = "",
)

/** A stretch of time at one price. */
@Serializable
data class PricePeriod(
    val start: Long,
    val end: Long,
    val price: Double,
    val slot: String = "",
    val profileName: String = "",
)

@Serializable
data class TempoView(val today: DayColor? = null, val tomorrow: DayColor? = null)

/** The price of electricity (`/api/installations/{id}/electricity/price`): null without a tariff. */
@Serializable
data class PriceView(
    val currency: String = "EUR",
    val now: PriceAt? = null,
    val periods: List<PricePeriod> = emptyList(),
    val tempo: TempoView? = null,
)

/** An hour of the forecast (`/api/weather`). */
@Serializable
data class WeatherHour(
    val ts: Long,
    val temperature: Double? = null,
    /** WMO weather code. */
    val code: Long? = null,
    val precipitationProbability: Double? = null,
)

@Serializable
data class WeatherReport(
    val place: String = "",
    val temperature: Double? = null,
    val apparent: Double? = null,
    val code: Long? = null,
    val wind: Double? = null,
    val todayMin: Double? = null,
    val todayMax: Double? = null,
    val hours: List<WeatherHour> = emptyList(),
)

@Serializable
data class SunTimes(val sunrise: Long? = null, val sunset: Long? = null)

/** The weather of the home (null without a city) and the sun of today. */
@Serializable
data class WeatherView(val weather: WeatherReport? = null, val sun: SunTimes? = null)
