package com.breezone.app.ui

import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import com.breezone.app.data.Installation
import com.breezone.app.data.Mode
import com.breezone.app.data.Zone
import com.breezone.app.resources.Res
import com.breezone.app.resources.days_names
import com.breezone.app.resources.installation_via
import com.breezone.app.resources.mode_aux_heating
import com.breezone.app.resources.mode_auto
import com.breezone.app.resources.mode_cooling
import com.breezone.app.resources.mode_dry
import com.breezone.app.resources.mode_fan
import com.breezone.app.resources.mode_heating
import com.breezone.app.resources.mode_stop
import com.breezone.app.resources.schedule_at
import com.breezone.app.resources.schedule_every_day
import com.breezone.app.resources.schedule_manual
import com.breezone.app.resources.schedule_weekdays
import com.breezone.app.resources.schedule_weekend
import com.breezone.app.resources.time_days_ago
import com.breezone.app.resources.time_hours_ago
import com.breezone.app.resources.time_in_days
import com.breezone.app.resources.time_in_hours
import com.breezone.app.resources.time_in_minutes
import com.breezone.app.resources.time_minutes_ago
import com.breezone.app.resources.time_never
import com.breezone.app.resources.time_now
import com.breezone.app.resources.time_soon
import com.breezone.app.ui.icons.FzIcons
import com.breezone.app.ui.theme.Fz
import kotlinx.datetime.TimeZone
import kotlinx.datetime.number
import kotlinx.datetime.toLocalDateTime
import org.jetbrains.compose.resources.StringResource
import org.jetbrains.compose.resources.stringArrayResource
import org.jetbrains.compose.resources.stringResource
import kotlin.math.abs
import kotlin.math.roundToInt
import kotlin.math.roundToLong
import kotlin.time.Clock
import kotlin.time.Instant

val Mode.labelRes: StringResource
    get() = when (this) {
        Mode.Stop -> Res.string.mode_stop
        Mode.Cooling -> Res.string.mode_cooling
        Mode.Heating -> Res.string.mode_heating
        Mode.Fan -> Res.string.mode_fan
        Mode.Dry -> Res.string.mode_dry
        Mode.AuxHeating -> Res.string.mode_aux_heating
        Mode.Auto -> Res.string.mode_auto
    }

@Composable
fun Mode.label(): String = stringResource(labelRes)

val Mode.icon: ImageVector
    get() = when (this) {
        Mode.Stop -> FzIcons.Power
        Mode.Cooling -> FzIcons.Snowflake
        Mode.Heating -> FzIcons.Flame
        Mode.Fan -> FzIcons.Fan
        Mode.Dry -> FzIcons.Droplets
        Mode.AuxHeating -> FzIcons.Heater
        Mode.Auto -> FzIcons.Sparkles
    }

@Composable
fun Mode.color(): Color = when (this) {
    Mode.Stop -> Fz.colors.muted
    Mode.Cooling -> Fz.colors.cool
    Mode.Heating, Mode.AuxHeating -> Fz.colors.heat
    Mode.Fan -> Fz.colors.fan
    Mode.Dry -> Fz.colors.dry
    Mode.Auto -> MaterialTheme.colorScheme.primary
}

/** Accent of a zone: its mode color when on, muted when off. */
@Composable
fun Zone.tone(): Color = if (on && mode != null) mode.color() else Fz.colors.muted

/** Temperature with a decimal comma and a degree sign ("19,5°"); numbers are not translated. */
fun formatTemp(value: Double?, digits: Int = 1): String {
    if (value == null) return "–"
    val factor = if (digits == 0) 1.0 else 10.0
    val rounded = (value * factor).roundToLong() / factor
    val text = if (digits == 0) rounded.roundToInt().toString() else {
        val whole = rounded.toLong()
        val decimal = abs(((rounded - whole) * 10).roundToInt())
        "${if (rounded < 0 && whole == 0L) "-" else ""}$whole,$decimal"
    }
    return "$text°"
}

/** Setpoint range and step accepted by every zone of a set (15–30° by 0.5 when they say nothing). */
data class SetpointLimits(val min: Double, val max: Double, val step: Double) {
    fun clamp(value: Double): Double = ((value / step).roundToLong() * step).coerceIn(min, max)

    companion object {
        private val DEFAULT = SetpointLimits(15.0, 30.0, 0.5)

        fun of(zones: List<Zone>): SetpointLimits {
            if (zones.isEmpty()) return DEFAULT
            val min = zones.maxOf { it.minTemp ?: DEFAULT.min }
            val max = zones.minOf { it.maxTemp ?: DEFAULT.max }
            val step = zones.maxOf { if (it.step > 0) it.step else DEFAULT.step }
            return if (min <= max) SetpointLimits(min, max, step) else DEFAULT
        }
    }
}

fun clampSetpoint(zone: Zone, value: Double): Double = SetpointLimits.of(listOf(zone)).clamp(value)

fun nowSeconds() = Clock.System.now().epochSeconds

@Composable
fun relativeTime(ts: Long?): String {
    if (ts == null) return stringResource(Res.string.time_never)
    val diff = ts - nowSeconds()
    val a = abs(diff)
    val future = diff > 0
    return when {
        a < 45 -> stringResource(if (future) Res.string.time_soon else Res.string.time_now)
        a < 3600 -> stringResource(if (future) Res.string.time_in_minutes else Res.string.time_minutes_ago, (a / 60.0).roundToInt())
        a < 86_400 -> stringResource(if (future) Res.string.time_in_hours else Res.string.time_hours_ago, (a / 3600.0).roundToInt())
        else -> stringResource(if (future) Res.string.time_in_days else Res.string.time_days_ago, (a / 86_400.0).roundToInt())
    }
}

/** "dd/MM/yyyy" in the phone's timezone (dates of a certificate). */
fun day(ts: Long): String {
    val t = Instant.fromEpochSeconds(ts).toLocalDateTime(TimeZone.currentSystemDefault())
    return "${t.day.toString().padStart(2, '0')}/${t.month.number.toString().padStart(2, '0')}/${t.year}"
}

/** "HH:mm" in the phone's timezone (a time of day, not translated). */
fun clock(ts: Long): String {
    val t = Instant.fromEpochSeconds(ts).toLocalDateTime(TimeZone.currentSystemDefault())
    return "${t.hour.toString().padStart(2, '0')}:${t.minute.toString().padStart(2, '0')}"
}

@Composable
fun scheduleText(days: List<Int>, time: String?): String {
    if (time == null) return stringResource(Res.string.schedule_manual)
    val names = stringArrayResource(Res.array.days_names)
    val d = days.sorted()
    val label = when {
        d.size == 7 -> stringResource(Res.string.schedule_every_day)
        d == listOf(1, 2, 3, 4, 5) -> stringResource(Res.string.schedule_weekdays)
        d == listOf(6, 7) -> stringResource(Res.string.schedule_weekend)
        else -> d.joinToString(", ") { names[it - 1] }
    }
    return stringResource(Res.string.schedule_at, label, time)
}

fun groupZones(zones: List<Zone>): List<Pair<String?, List<Zone>>> {
    val groups = linkedMapOf<String?, MutableList<Zone>>()
    zones.forEach { groups.getOrPut(it.group) { mutableListOf() }.add(it) }
    return groups.map { it.key to it.value }
}

data class ModePlacement(
    /** Master zones whose system lies entirely in one group, by group name. */
    val byGroup: Map<String?, List<Zone>>,
    /** Master zones of systems spanning several groups: shown in the installation header. */
    val header: List<Zone>,
    val systems: Int,
)

/**
 * The master zone of each system (it carries the system's mode), placed on the row of the group
 * holding all the system's zones, or in the header when the system spans several groups.
 */
fun systemModePlacement(zones: List<Zone>): ModePlacement {
    val masters = zones.filter { it.isMaster && it.modes.isNotEmpty() }.distinctBy { it.systemId }
    val byGroup = linkedMapOf<String?, MutableList<Zone>>()
    val header = mutableListOf<Zone>()
    masters.forEach { m ->
        val groups = zones.filter { it.systemId == m.systemId }.map { it.group }.distinct()
        if (groups.size == 1) byGroup.getOrPut(groups.first()) { mutableListOf() }.add(m) else header.add(m)
    }
    return ModePlacement(byGroup, header, masters.size)
}

/** A stable hue per installation name (colored cards like the official app). */
fun hueOf(seed: String): Float {
    var h = 0
    seed.forEach { h = (h * 31 + it.code) % 360 }
    return h.toFloat()
}

/** Where an installation is: its city and, when reached through another Breezone server, its name. */
@Composable
fun installationPlace(inst: Installation): String {
    val via = inst.node?.let { stringResource(Res.string.installation_via, it.name) }
    return listOfNotNull(inst.location, via).joinToString(" · ").ifEmpty { "${inst.host}:${inst.port}" }
}
