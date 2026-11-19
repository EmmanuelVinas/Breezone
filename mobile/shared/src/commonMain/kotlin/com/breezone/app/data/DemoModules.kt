package com.breezone.app.data

import com.breezone.app.resources.Res
import com.breezone.app.resources.demo_automation_heater
import com.breezone.app.resources.demo_automation_heater_if
import com.breezone.app.resources.demo_automation_heater_then
import com.breezone.app.resources.demo_automation_heater_when
import com.breezone.app.resources.demo_automation_home
import com.breezone.app.resources.demo_automation_home_if
import com.breezone.app.resources.demo_automation_home_then
import com.breezone.app.resources.demo_automation_home_then2
import com.breezone.app.resources.demo_automation_home_when
import com.breezone.app.resources.demo_automation_shutters
import com.breezone.app.resources.demo_automation_shutters_then
import com.breezone.app.resources.demo_automation_shutters_when
import com.breezone.app.resources.demo_charger
import com.breezone.app.resources.demo_cover_bedroom
import com.breezone.app.resources.demo_cover_kitchen
import com.breezone.app.resources.demo_cover_living
import com.breezone.app.resources.demo_cover_office
import com.breezone.app.resources.demo_inverter
import com.breezone.app.resources.demo_light_bedside
import com.breezone.app.resources.demo_light_kitchen
import com.breezone.app.resources.demo_light_living
import com.breezone.app.resources.demo_light_office
import com.breezone.app.resources.demo_meter_grid
import com.breezone.app.resources.demo_meter_solar
import com.breezone.app.resources.demo_remote
import com.breezone.app.resources.demo_room_bathroom
import com.breezone.app.resources.demo_room_garage
import com.breezone.app.resources.demo_room_kitchen
import com.breezone.app.resources.demo_scene_dinner
import com.breezone.app.resources.demo_scene_reading
import com.breezone.app.resources.demo_scene_relax
import com.breezone.app.resources.demo_ventilation
import com.breezone.app.resources.demo_water_heater
import com.breezone.app.resources.demo_zone_bedroom1
import com.breezone.app.resources.demo_zone_living
import com.breezone.app.resources.demo_zone_office
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.doubleOrNull
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull
import kotlinx.serialization.json.put
import kotlinx.serialization.json.contentOrNull
import org.jetbrains.compose.resources.getString
import kotlin.math.PI
import kotlin.math.max
import kotlin.math.round
import kotlin.math.sin
import kotlin.time.Clock
import kotlinx.datetime.LocalDateTime
import kotlinx.datetime.TimeZone
import kotlinx.datetime.atStartOfDayIn
import kotlinx.datetime.toLocalDateTime
import kotlin.time.Instant

/**
 * The modules of the demonstration: lights, shutters, energy (solar, grid, water heater,
 * charger), ventilation, a remote, the weather and a few automations, so that every screen of
 * the app has something to show. Their measures follow the time of day.
 */
internal object DemoModules {
    private const val HUE = 1L
    private const val HOMEKIT = 2L
    private const val SHELLY = 3L
    private const val TRYDAN = 4L
    private const val RENSON = 5L
    private const val HOYMILES = 6L

    private const val LIVING = 1L
    private const val OFFICE = 2L
    private const val BEDROOM = 3L
    private const val KITCHEN = 4L
    private const val BATHROOM = 5L
    private const val GARAGE = 6L

    private const val GRID = 121L
    private const val SOLAR = 122L
    private const val HEATER = 123L
    private const val CHARGER = 124L
    private const val INVERTER = 125L

    private var devices: MutableList<DeviceView>? = null
    private var rooms: List<Room> = emptyList()
    private var scenes: List<SceneView> = emptyList()
    private var automations = mutableListOf<Automation>()

    private fun now() = Clock.System.now().epochSeconds
    private val zone get() = TimeZone.currentSystemDefault()
    private fun hourOf(ts: Long): Double {
        val t = Instant.fromEpochSeconds(ts).toLocalDateTime(zone)
        return t.hour + t.minute / 60.0
    }
    private fun midnight(): Long = Clock.System.now().toLocalDateTime(zone).date.atStartOfDayIn(zone).epochSeconds
    private fun r1(v: Double) = round(v * 10) / 10

    /** The sun on the roof (W): a bell from 7:30 to 18:30, a few clouds. */
    private fun solar(ts: Long): Double {
        val h = hourOf(ts)
        if (h < 7.5 || h > 18.5) return 0.0
        val bell = sin((h - 7.5) / 11.0 * PI)
        val clouds = 1.0 - 0.18 * max(0.0, sin(ts / 1700.0))
        return round(4200 * bell * bell * clouds)
    }

    /** The water heater runs on the surplus around noon. */
    private fun heaterOn(ts: Long) = hourOf(ts) in 11.5..14.5
    /** The home without the water heater and the charger (W). */
    private fun base(ts: Long): Double {
        val h = hourOf(ts)
        var w = 380.0 + 60 * sin(ts / 900.0)
        if (h in 6.5..8.5) w += 1300
        if (h in 12.0..13.5) w += 600
        if (h in 18.5..21.5) w += 1700
        return w
    }

    /** The car charges on what the sun leaves, from 6 A, in the afternoon (W). */
    private fun carPower(ts: Long): Double {
        if (hourOf(ts) !in 11.0..16.5) return 0.0
        val left = solar(ts) - base(ts) - (if (heaterOn(ts)) 1800.0 else 0.0) - 250
        val amps = (left / 230).toInt().coerceAtMost(16)
        return if (amps >= 6) amps * 230.0 else 0.0
    }
    private fun charging(ts: Long) = carPower(ts) > 0

    /** What the home draws (W), the water heater and the charger included. */
    private fun home(ts: Long): Double =
        round(base(ts) + (if (heaterOn(ts)) 1800.0 else 0.0) + carPower(ts))

    private fun state(vararg pairs: Pair<String, Any?>): JsonObject = buildJsonObject {
        for ((k, v) in pairs) when (v) {
            null -> {}
            is Boolean -> put(k, v)
            is Number -> put(k, v)
            is String -> put(k, v)
        }
    }

    private suspend fun ensure() {
        if (devices != null) return
        rooms = listOf(
            Room(LIVING, getString(Res.string.demo_zone_living), 0),
            Room(KITCHEN, getString(Res.string.demo_room_kitchen), 1),
            Room(OFFICE, getString(Res.string.demo_zone_office), 2),
            Room(BEDROOM, getString(Res.string.demo_zone_bedroom1), 3),
            Room(BATHROOM, getString(Res.string.demo_room_bathroom), 4),
            Room(GARAGE, getString(Res.string.demo_room_garage), 5),
        )
        fun dev(id: Long, kind: String, traits: List<String>, source: String, sourceId: Long, name: String, room: Long?, role: String? = null, s: JsonObject) =
            DeviceView(
                id = id, kind = kind, traits = traits, source = source, sourceId = sourceId, reference = JsonPrimitive("demo-$id"),
                name = name, roomId = room, role = role, position = id, state = s, canControl = true,
            )
        val light = listOf("light@1")
        val cover = listOf("cover.position@1")
        devices = mutableListOf(
            dev(101, "light", light, "hue", HUE, getString(Res.string.demo_light_living), LIVING,
                s = state("on" to true, "brightness" to 70, "mode" to "color", "color" to "#ffb35c", "color_temp" to 2700, "color_temp_min" to 2000, "color_temp_max" to 6500, "reachable" to true)),
            dev(102, "light", light, "hue", HUE, getString(Res.string.demo_light_kitchen), KITCHEN,
                s = state("on" to true, "brightness" to 85, "mode" to "white", "color_temp" to 3000, "color_temp_min" to 2000, "color_temp_max" to 6500, "reachable" to true)),
            dev(103, "light", light, "hue", HUE, getString(Res.string.demo_light_bedside), BEDROOM,
                s = state("on" to false, "brightness" to 40, "mode" to "white", "color_temp" to 2400, "color_temp_min" to 2000, "color_temp_max" to 6500, "reachable" to true)),
            dev(104, "light", light, "hue", HUE, getString(Res.string.demo_light_office), OFFICE,
                s = state("on" to true, "brightness" to 100, "mode" to "white", "color_temp" to 4500, "color_temp_min" to 2000, "color_temp_max" to 6500, "reachable" to true)),
            dev(111, "shutter", cover, "homekit", HOMEKIT, getString(Res.string.demo_cover_living), LIVING,
                s = state("position" to 100, "target" to 100, "moving" to "stopped", "stoppable" to true)),
            dev(112, "shutter", cover, "homekit", HOMEKIT, getString(Res.string.demo_cover_kitchen), KITCHEN,
                s = state("position" to 60, "target" to 60, "moving" to "stopped", "stoppable" to true)),
            dev(113, "shutter", cover, "homekit", HOMEKIT, getString(Res.string.demo_cover_bedroom), BEDROOM,
                s = state("position" to 0, "target" to 0, "moving" to "stopped", "stoppable" to true)),
            dev(114, "shutter", cover, "homekit", HOMEKIT, getString(Res.string.demo_cover_office), OFFICE,
                s = state("position" to 100, "target" to 100, "moving" to "stopped", "stoppable" to true)),
            dev(GRID, "energy.power", listOf("energy.power@1", "energy.meter@1"), "shelly", SHELLY, getString(Res.string.demo_meter_grid), GARAGE, "grid", state()),
            dev(SOLAR, "energy.power", listOf("energy.power@1"), "shelly", SHELLY, getString(Res.string.demo_meter_solar), GARAGE, "solar", state()),
            dev(HEATER, "switch.relay", listOf("switch.relay@1", "energy.power@1"), "shelly", SHELLY, getString(Res.string.demo_water_heater), GARAGE, "consumption", state()),
            dev(CHARGER, "ev.charger", listOf("ev.charger@1", "energy.power@1"), "trydan", TRYDAN, getString(Res.string.demo_charger), GARAGE, s = state()),
            dev(INVERTER, "solar.inverter", listOf("solar.inverter@1", "energy.power@1"), "hoymiles", HOYMILES, getString(Res.string.demo_inverter), GARAGE, s = state()),
            dev(131, "ventilation.unit", listOf("ventilation.unit@1"), "renson", RENSON, getString(Res.string.demo_ventilation), null,
                s = state("flow" to 118, "power" to 17, "air_quality_index" to 2, "main_pollutant" to "humidity", "worst_room" to getString(Res.string.demo_room_bathroom), "boost" to false, "sensors" to true)),
            dev(132, "ventilation.room", listOf("ventilation.room@1"), "renson", RENSON, getString(Res.string.demo_room_bathroom), BATHROOM,
                s = state("flow" to 52, "nominal" to 75, "level" to 70, "profile" to "health", "boost" to false, "humidity" to 68, "temperature" to 21.4, "air_quality_index" to 2, "main_pollutant" to "humidity")),
            dev(133, "ventilation.room", listOf("ventilation.room@1"), "renson", RENSON, getString(Res.string.demo_room_kitchen), KITCHEN,
                s = state("flow" to 41, "nominal" to 75, "level" to 55, "profile" to "health", "boost" to false, "humidity" to 51, "temperature" to 20.8, "co2" to 720, "voc" to 180, "air_quality_index" to 1)),
            dev(134, "ventilation.room", listOf("ventilation.room@1"), "renson", RENSON, getString(Res.string.demo_zone_bedroom1), BEDROOM,
                s = state("flow" to 25, "nominal" to 50, "level" to 50, "profile" to "eco", "boost" to false, "humidity" to 47, "temperature" to 18.6, "co2" to 610, "air_quality_index" to 1)),
            dev(141, "remote", listOf("button@1"), "homekit", HOMEKIT, getString(Res.string.demo_remote), LIVING, s = state("buttons" to 4)),
        )
        scenes = listOf(
            SceneView(HUE, "reading", getString(Res.string.demo_scene_reading), rooms[0].name, listOf(101)),
            SceneView(HUE, "relax", getString(Res.string.demo_scene_relax), rooms[0].name, listOf(101, 102)),
            SceneView(HUE, "dinner", getString(Res.string.demo_scene_dinner), rooms[1].name, listOf(102)),
        )
        val t = now()
        automations = mutableListOf(
            Automation(
                id = 1, name = getString(Res.string.demo_automation_shutters),
                summary = AutomationSummary(
                    triggers = listOf(getString(Res.string.demo_automation_shutters_when)),
                    actions = listOf(getString(Res.string.demo_automation_shutters_then)),
                ),
                status = AutomationStatus(nextAt = midnight() + 18 * 3600),
                lastRunAt = t - 20 * 3600,
            ),
            Automation(
                id = 2, name = getString(Res.string.demo_automation_heater),
                summary = AutomationSummary(
                    triggers = listOf(getString(Res.string.demo_automation_heater_when)),
                    conditions = listOf(getString(Res.string.demo_automation_heater_if)),
                    joined = "et",
                    actions = listOf(getString(Res.string.demo_automation_heater_then)),
                ),
                status = AutomationStatus(value = max(0.0, solar(t) - home(t)), unit = "W", conditions = listOf(true)),
                lastRunAt = midnight() + (11.5 * 3600).toLong(),
            ),
            Automation(
                id = 3, name = getString(Res.string.demo_automation_home),
                summary = AutomationSummary(
                    triggers = listOf(getString(Res.string.demo_automation_home_when)),
                    conditions = listOf(getString(Res.string.demo_automation_home_if)),
                    joined = "et",
                    actions = listOf(getString(Res.string.demo_automation_home_then), getString(Res.string.demo_automation_home_then2)),
                ),
                status = AutomationStatus(conditions = listOf(hourOf(t) > 18.0)),
                lastRunAt = t - 22 * 3600,
            ),
        )
    }

    /** The measures of now, written into the devices. */
    private fun refresh() {
        val t = now()
        val sun = solar(t)
        val total = home(t)
        val car = charging(t)
        devices = devices?.map { d ->
            when (d.id) {
                GRID -> d.copy(state = state("power" to total - sun, "energy_in" to 8341.2, "energy_out" to 1922.6))
                SOLAR -> d.copy(state = state("power" to sun))
                INVERTER -> d.copy(state = state("power" to sun, "energy_today" to r1(energyOf(::solar)), "energy_total" to 6123.4, "temperature" to 38.5, "panels" to 4))
                HEATER -> {
                    val on = d.state?.get("on")?.jsonPrimitive?.booleanOrNull ?: heaterOn(t)
                    d.copy(state = state("on" to on, "power" to if (on) 1800.0 else 0.0))
                }
                CHARGER -> d.copy(state = state(
                    "status" to if (car) "charging" else "plugged", "vehicle" to true, "paused" to false,
                    "power" to carPower(t), "limit" to (carPower(t) / 230).toInt().coerceAtLeast(6), "limit_min" to 6, "limit_max" to 16, "phases" to 1,
                    "session_energy" to r1(energyOf(::carPower)), "auto_limit" to true,
                ))
                else -> d
            }
        }?.toMutableList()
    }

    /** Energy since midnight (kWh) of a power over the day. */
    private fun energyOf(power: (Long) -> Double): Double {
        var wh = 0.0
        var ts = midnight()
        val end = now()
        while (ts < end) {
            wh += power(ts) * 0.25
            ts += 900
        }
        return wh / 1000
    }

    private fun live(): EnergyLive {
        val t = now()
        val sun = solar(t)
        val total = home(t)
        val heater = if (heaterOn(t)) 1800.0 else 0.0
        val car = carPower(t)
        return EnergyLive(
            roles = EnergyRoles(grid = true, solar = true, consumption = true),
            grid = total - sun, solar = sun, home = total,
            consumers = listOf(
                EnergyConsumer(devices!!.first { it.id == HEATER }.name, HEATER, GARAGE, heater),
                EnergyConsumer(devices!!.first { it.id == CHARGER }.name, CHARGER, GARAGE, car),
            ),
            other = total - heater - car,
        )
    }

    private fun day(): EnergyDay {
        val start = midnight()
        val end = now()
        val points = generateSequence(start) { it + 900 }.takeWhile { it <= end }.map { ts ->
            EnergyPoint(ts, grid = home(ts) - solar(ts), solar = solar(ts), home = home(ts))
        }.toList()
        val imported = points.sumOf { max(0.0, it.grid ?: 0.0) } * 0.25 / 1000
        val exported = points.sumOf { max(0.0, -(it.grid ?: 0.0)) } * 0.25 / 1000
        val production = points.sumOf { it.solar ?: 0.0 } * 0.25 / 1000
        val consumption = points.sumOf { it.home ?: 0.0 } * 0.25 / 1000
        val heater = energyOf { if (heaterOn(it)) 1800.0 else 0.0 }
        val car = energyOf(::carPower)
        return EnergyDay(
            date = Clock.System.now().toLocalDateTime(zone).date.toString(),
            bucket = 900,
            points = points,
            totals = DayEnergy(
                day = Clock.System.now().toLocalDateTime(zone).date.toString(),
                imported = r1(imported), exported = r1(exported), production = r1(production), consumption = r1(consumption),
                selfConsumed = r1(production - exported),
                consumers = listOf(
                    EnergyConsumer(devices!!.first { it.id == HEATER }.name, HEATER, GARAGE, energy = r1(heater)),
                    EnergyConsumer(devices!!.first { it.id == CHARGER }.name, CHARGER, GARAGE, energy = r1(car)),
                ),
                other = r1(consumption - heater - car),
            ),
        )
    }

    /** The fields of a device that have a history, and their value at a time. */
    private fun fields(d: DeviceView): Map<String, (Long) -> Double> = when {
        d.id == GRID -> mapOf("power" to { ts -> home(ts) - solar(ts) })
        d.id == SOLAR || d.id == INVERTER -> mapOf("power" to ::solar)
        d.id == HEATER -> mapOf("power" to { ts -> if (heaterOn(ts)) 1800.0 else 0.0 })
        d.id == CHARGER -> mapOf("power" to ::carPower)
        d.has("ventilation.room@1") -> {
            val base = d.state?.get("humidity")?.jsonPrimitive?.doubleOrNull ?: 50.0
            mapOf(
                "humidity" to { ts -> r1(base - 6 + 8 * max(0.0, sin((hourOf(ts) - 6) / 3 * PI)) + 2 * sin(ts / 5000.0)) },
                "flow" to { ts -> r1((d.state?.get("flow")?.jsonPrimitive?.doubleOrNull ?: 40.0) * (0.7 + 0.3 * sin(ts / 7000.0))) },
            )
        }
        d.has("ventilation.unit@1") -> mapOf("flow" to { ts -> r1(110 + 25 * sin(ts / 6000.0)) })
        else -> emptyMap()
    }

    private fun history(d: DeviceView, hours: Int, bucket: Int): DeviceHistory {
        val end = now()
        val step = bucket.toLong().coerceAtLeast(300)
        val start = end - hours * 3600L
        val series = fields(d).mapValues { (_, f) ->
            generateSequence(start - start % step) { it + step }.takeWhile { it <= end }.map { listOf(it.toDouble(), r1(f(it))) }.toList()
        }
        return DeviceHistory(bucket = step, series = series, oldest = end - 400L * 86_400)
    }

    private fun weather(): WeatherView {
        val t = now()
        val base = midnight()
        val temp = { ts: Long -> r1(10.5 + 5.5 * sin((hourOf(ts) - 9) / 24 * 2 * PI)) }
        return WeatherView(
            weather = WeatherReport(
                place = "Annecy", temperature = temp(t), apparent = r1(temp(t) - 1.5), code = 2, wind = 11.0,
                todayMin = 6.0, todayMax = 16.0,
                hours = (1..12).map { i ->
                    val ts = t - t % 3600 + i * 3600L
                    WeatherHour(ts, temp(ts), if (i in 5..7) 3 else if (i > 9) 61 else 1, if (i > 9) 60.0 else 5.0)
                },
            ),
            sun = SunTimes(base + (7.6 * 3600).toLong(), base + (17.4 * 3600).toLong()),
        )
    }

    /** Applies an action to a device; the device as it is after it, or null when unknown. */
    private fun act(id: Long, input: DeviceActionInput): DeviceView? {
        val list = devices ?: return null
        val index = list.indexOfFirst { it.id == id }.takeIf { it >= 0 } ?: return null
        val d = list[index]
        val s = (d.state ?: JsonObject(emptyMap())).toMutableMap()
        val p = input.params
        fun num(k: String) = p[k]?.jsonPrimitive?.doubleOrNull
        when ("${input.trait}.${input.action}") {
            "light.set", "switch.relay.set" -> {
                p["on"]?.jsonPrimitive?.booleanOrNull?.let { s["on"] = JsonPrimitive(it) }
                num("brightness")?.let { s["brightness"] = JsonPrimitive(it.toLong()); s["on"] = JsonPrimitive(true) }
                num("color_temp")?.let { s["color_temp"] = JsonPrimitive(it.toLong()); s["mode"] = JsonPrimitive("white") }
                p["color"]?.jsonPrimitive?.contentOrNull?.let { s["color"] = JsonPrimitive(it); s["mode"] = JsonPrimitive("color") }
                if (d.id == HEATER) s["power"] = JsonPrimitive(if (s["on"]?.jsonPrimitive?.booleanOrNull == true) 1800.0 else 0.0)
            }
            "cover.position.set" -> num("position")?.let {
                s["position"] = JsonPrimitive(it.toLong()); s["target"] = JsonPrimitive(it.toLong()); s["moving"] = JsonPrimitive("stopped")
            }
            "cover.position.stop" -> s["moving"] = JsonPrimitive("stopped")
            "button.press" -> {
                s["last_button"] = JsonPrimitive(p["button"]?.jsonPrimitive?.longOrNull ?: 1)
                s["last_press"] = JsonPrimitive("single")
                s["last_at"] = JsonPrimitive(now())
            }
            "ventilation.room.boost", "ventilation.unit.boost" -> s["boost"] = JsonPrimitive(true)
            else -> {}
        }
        val next = d.copy(state = JsonObject(s))
        list[index] = next
        return next
    }

    /** Answers a request of the modules' API; null when it is not one of theirs. */
    suspend fun handle(path: String, query: (String) -> String?, body: suspend () -> String): String? {
        ensure()
        refresh()
        val parts = path.trim('/').split('/')
        return when {
            path == "/api/devices" -> FzJson.encodeToString(devices!!.toList())
            path == "/api/rooms" -> FzJson.encodeToString(rooms)
            path == "/api/scenes" -> FzJson.encodeToString(scenes)
            path.startsWith("/api/scenes/") -> "{}"
            path == "/api/devices/charted" ->
                FzJson.encodeToString(devices!!.mapNotNull { d -> fields(d).keys.toList().takeIf { it.isNotEmpty() }?.let { Charted(d.id, it) } })
            parts.size == 4 && parts[1] == "devices" && parts[3] == "history" -> {
                val d = devices!!.firstOrNull { it.id == parts[2].toLongOrNull() } ?: return null
                val hours = query("hours")?.toIntOrNull() ?: 24
                val bucket = query("bucket")?.toIntOrNull() ?: (hours * 3600 / (query("points")?.toIntOrNull() ?: 96))
                FzJson.encodeToString(history(d, hours.coerceIn(1, 24 * 400), bucket))
            }
            parts.size == 4 && parts[1] == "devices" && parts[3] == "actions" -> {
                val input = FzJson.decodeFromString(DeviceActionInput.serializer(), body())
                act(parts[2].toLongOrNull() ?: return null, input)?.let { FzJson.encodeToString(it) }
            }
            path == "/api/energy/live" -> FzJson.encodeToString(live())
            path == "/api/energy/day" -> FzJson.encodeToString(day())
            path == "/api/weather" -> FzJson.encodeToString(weather())
            path == "/api/presence" -> FzJson.encodeToString(Presence(me = true, someoneHome = true))
            path == "/api/automations" -> FzJson.encodeToString(automations.toList())
            path == "/api/automations/settings" -> FzJson.encodeToString(AutomationSettings())
            path == "/api/automations/unreadable" -> "[]"
            parts.size == 4 && parts[1] == "automations" && parts[3] == "enabled" -> {
                val id = parts[2].toLongOrNull()
                val on = FzJson.decodeFromString(EnabledBody.serializer(), body()).enabled
                automations = automations.map { if (it.id == id) it.copy(enabled = on) else it }.toMutableList()
                automations.firstOrNull { it.id == id }?.let { FzJson.encodeToString(it) }
            }
            parts.size == 4 && parts[1] == "automations" && parts[3] == "run" -> FzJson.encodeToString(AutomationRunResult())
            parts.size == 4 && parts[1] == "automations" && parts[3] == "runs" -> "[]"
            else -> null
        }
    }
}
