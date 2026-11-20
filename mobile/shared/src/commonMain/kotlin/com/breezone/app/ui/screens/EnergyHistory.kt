package com.breezone.app.ui.screens

import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material3.DatePicker
import androidx.compose.material3.DatePickerDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.SelectableDates
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberDatePickerState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.breezone.app.data.DayEnergy
import com.breezone.app.data.EnergyDay
import com.breezone.app.data.EnergyRoles
import com.breezone.app.data.Hub
import com.breezone.app.resources.Res
import com.breezone.app.resources.common_cancel
import com.breezone.app.resources.common_ok
import com.breezone.app.resources.days_names
import com.breezone.app.resources.energy_consumed
import com.breezone.app.resources.energy_imported
import com.breezone.app.resources.energy_next_day
import com.breezone.app.resources.energy_no_data
import com.breezone.app.resources.energy_period_day
import com.breezone.app.resources.energy_period_month
import com.breezone.app.resources.energy_period_week
import com.breezone.app.resources.energy_period_year
import com.breezone.app.resources.energy_period_years
import com.breezone.app.resources.energy_pick_day
import com.breezone.app.resources.energy_previous_day
import com.breezone.app.resources.energy_produced
import com.breezone.app.resources.energy_today
import com.breezone.app.resources.months_short
import com.breezone.app.ui.components.EnergyBar
import com.breezone.app.ui.components.EnergyBarsChart
import com.breezone.app.ui.components.PowerDayChart
import com.breezone.app.ui.components.PowerSeries
import com.breezone.app.ui.icons.FzIcons
import com.breezone.app.ui.theme.Fz
import kotlinx.coroutines.delay
import kotlinx.datetime.DateTimeUnit
import kotlinx.datetime.LocalDate
import kotlinx.datetime.TimeZone
import kotlinx.datetime.atStartOfDayIn
import kotlinx.datetime.daysUntil
import kotlinx.datetime.minus
import kotlinx.datetime.number
import kotlinx.datetime.plus
import kotlinx.datetime.toLocalDateTime
import org.jetbrains.compose.resources.StringResource
import org.jetbrains.compose.resources.stringArrayResource
import org.jetbrains.compose.resources.stringResource
import kotlin.time.Clock
import kotlin.time.Instant

/** The periods of the energy page, as on the web: a day, the last 7 or 30 days, a year, five. */
private enum class EnergyPeriod(val days: Int, val label: StringResource) {
    Day(1, Res.string.energy_period_day),
    Week(7, Res.string.energy_period_week),
    Month(30, Res.string.energy_period_month),
    Year(365, Res.string.energy_period_year),
    Years(5 * 365 + 1, Res.string.energy_period_years),
}

private fun today(): LocalDate = Clock.System.now().toLocalDateTime(TimeZone.currentSystemDefault()).date

/** The energy over a period chosen above: the day's curves (any day), or the days in bars. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun EnergyHistoryCard(hub: Hub, serverId: String, roles: EnergyRoles) {
    var period by remember { mutableStateOf(EnergyPeriod.Day) }
    var date by remember { mutableStateOf(today()) }
    var picking by remember { mutableStateOf(false) }
    var firstDay by remember { mutableStateOf<LocalDate?>(null) }
    var day by remember { mutableStateOf<EnergyDay?>(null) }
    var days by remember { mutableStateOf<List<DayEnergy>?>(null) }

    LaunchedEffect(serverId) {
        firstDay = runCatching { hub.api(serverId)?.energyRange()?.firstDay?.let(LocalDate::parse) }.getOrNull()
    }
    LaunchedEffect(serverId, period, date) {
        if (period != EnergyPeriod.Day) return@LaunchedEffect
        day = null
        while (true) {
            runCatching { hub.api(serverId)?.energyDay(date.toString()) }.getOrNull()?.let { day = it }
            if (date != today()) break
            delay(60_000)
        }
    }
    LaunchedEffect(serverId, period) {
        if (period == EnergyPeriod.Day) return@LaunchedEffect
        days = null
        val count = if (period == EnergyPeriod.Week) 30 else period.days
        days = runCatching { hub.api(serverId)?.energyDays(count) }.getOrNull()?.let { all ->
            all.takeLast(period.days).filter { d -> period != EnergyPeriod.Years || firstDay == null || d.day >= firstDay.toString() }
        }
    }

    val age = firstDay?.daysUntil(today()) ?: 0
    val periods = EnergyPeriod.entries.filter { it.days <= 30 || (it == EnergyPeriod.Year && age > 30) || (it == EnergyPeriod.Years && age > 365) }
    val sun = Fz.colors.warning
    val home = MaterialTheme.colorScheme.onSurface

    SectionCard {
        if (periods.size > 1) {
            Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                periods.forEach { p -> FilterChip(selected = period == p, onClick = { period = p }, label = { Text(stringResource(p.label)) }) }
            }
        }
        if (period == EnergyPeriod.Day) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Row(Modifier.weight(1f).clickable { picking = true }.padding(vertical = 6.dp), verticalAlignment = Alignment.CenterVertically) {
                    SectionTitle(if (date == today()) stringResource(Res.string.energy_today) else dayLabel(date))
                    Spacer(Modifier.width(6.dp))
                    Icon(FzIcons.CalendarClock, stringResource(Res.string.energy_pick_day), Modifier.size(16.dp), tint = Fz.colors.muted)
                }
                IconButton(onClick = { date = date.minus(1, DateTimeUnit.DAY) }, enabled = firstDay == null || date > firstDay!!) {
                    Icon(FzIcons.ChevronLeft, stringResource(Res.string.energy_previous_day))
                }
                IconButton(onClick = { date = date.plus(1, DateTimeUnit.DAY) }, enabled = date < today()) {
                    Icon(FzIcons.ChevronRight, stringResource(Res.string.energy_next_day))
                }
            }
            val d = day
            val start = d?.points?.firstOrNull()?.ts ?: date.atStartOfDayIn(TimeZone.currentSystemDefault()).epochSeconds
            when {
                d == null -> Box(Modifier.fillMaxWidth().height(170.dp), contentAlignment = Alignment.Center) { LoadingWave() }
                d.points.none { it.home != null || it.grid != null } -> Box(Modifier.fillMaxWidth().height(170.dp), contentAlignment = Alignment.Center) {
                    Muted(stringResource(Res.string.energy_no_data))
                }
                else -> PowerDayChart(
                    listOf(
                        PowerSeries(d.points.mapNotNull { p -> p.solar?.let { p.ts to it } }, sun, area = true),
                        PowerSeries(d.points.mapNotNull { p -> p.home?.let { p.ts to it } }, home, area = false),
                    ),
                    start,
                    Modifier.fillMaxWidth().height(170.dp),
                )
            }
            d?.let { Totals(it.totals, roles) }
        } else {
            val list = days
            if (list == null) {
                Box(Modifier.fillMaxWidth().height(170.dp), contentAlignment = Alignment.Center) { LoadingWave() }
            } else {
                val months = stringArrayResource(Res.array.months_short)
                val bars = remember(list, period, months) { barsOf(list, period, months) }
                EnergyBarsChart(bars, home.copy(alpha = 0.75f), sun, Modifier.fillMaxWidth().height(170.dp))
                Totals(total(list), roles)
            }
        }
    }

    if (picking) {
        // The picker counts in UTC milliseconds: the day at midnight UTC.
        fun millis(d: LocalDate) = d.atStartOfDayIn(TimeZone.UTC).toEpochMilliseconds()
        val until = millis(today())
        val from = firstDay?.let(::millis)
        val state = rememberDatePickerState(
            initialSelectedDateMillis = millis(date),
            selectableDates = remember(from, until) {
                object : SelectableDates {
                    override fun isSelectableDate(utcTimeMillis: Long) = utcTimeMillis <= until && (from == null || utcTimeMillis >= from)
                }
            },
        )
        DatePickerDialog(
            onDismissRequest = { picking = false },
            confirmButton = {
                TextButton(onClick = {
                    state.selectedDateMillis?.let { date = Instant.fromEpochMilliseconds(it).toLocalDateTime(TimeZone.UTC).date }
                    picking = false
                }) { Text(stringResource(Res.string.common_ok)) }
            },
            dismissButton = { TextButton(onClick = { picking = false }) { Text(stringResource(Res.string.common_cancel)) } },
        ) { DatePicker(state) }
    }
}

@Composable
private fun Totals(t: DayEnergy, roles: EnergyRoles) {
    Row(Modifier.fillMaxWidth()) {
        if (roles.solar && t.production != null) Stat(stringResource(Res.string.energy_produced), formatEnergy(t.production), Fz.colors.warning, Modifier.weight(1f))
        Stat(stringResource(Res.string.energy_consumed), formatEnergy(t.consumption), modifier = Modifier.weight(1f))
        if (t.imported != null) Stat(stringResource(Res.string.energy_imported), formatEnergy(t.imported), modifier = Modifier.weight(1f))
    }
}

/** "Lun 17/11": the day of the week and the date. */
@Composable
private fun dayLabel(d: LocalDate): String {
    val names = stringArrayResource(Res.array.days_names)
    val year = if (d.year != today().year) "/${d.year}" else ""
    return "${names.getOrElse(d.dayOfWeek.ordinal) { "" }} ${d.day.toString().padStart(2, '0')}/${d.month.number.toString().padStart(2, '0')}$year"
}

/** A bar per day; over a year or more, a bar per month. */
private fun barsOf(list: List<DayEnergy>, period: EnergyPeriod, months: List<String>): List<EnergyBar> {
    if (period == EnergyPeriod.Week || period == EnergyPeriod.Month) {
        return list.map { d ->
            val parts = d.day.split('-')
            EnergyBar("${parts.getOrElse(2) { "" }}/${parts.getOrElse(1) { "" }}", d.consumption ?: 0.0, d.production)
        }
    }
    return list.groupBy { it.day.take(7) }.map { (month, ds) ->
        val m = month.substringAfter('-').toIntOrNull() ?: 1
        val label = months.getOrElse(m - 1) { month } + if (period == EnergyPeriod.Years) " ${month.take(4).takeLast(2)}" else ""
        EnergyBar(label, ds.sumOf { it.consumption ?: 0.0 }, ds.mapNotNull { it.production }.takeIf { it.isNotEmpty() }?.sum())
    }
}

/** The sum of days. */
private fun total(list: List<DayEnergy>): DayEnergy {
    fun sum(f: (DayEnergy) -> Double?) = list.mapNotNull(f).takeIf { it.isNotEmpty() }?.sum()
    return DayEnergy(
        imported = sum { it.imported },
        exported = sum { it.exported },
        production = sum { it.production },
        consumption = sum { it.consumption },
        selfConsumed = sum { it.selfConsumed },
    )
}
