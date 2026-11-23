package com.breezone.app.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderState
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.MutableState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.breezone.app.data.ChargeDay
import com.breezone.app.data.ChargeMode
import com.breezone.app.data.ChargeNext
import com.breezone.app.data.ChargeNowInput
import com.breezone.app.data.ChargingView
import com.breezone.app.data.DayColor
import com.breezone.app.data.DeviceView
import com.breezone.app.data.EvChargerState
import com.breezone.app.data.Hub
import com.breezone.app.resources.Res
import com.breezone.app.resources.charging_any_profile
import com.breezone.app.resources.charging_back_to_profile
import com.breezone.app.resources.charging_day_color
import com.breezone.app.resources.charging_in_progress
import com.breezone.app.resources.charging_kw
import com.breezone.app.resources.charging_kw_max
import com.breezone.app.resources.charging_kwh_unit
import com.breezone.app.resources.charging_legend_max
import com.breezone.app.resources.charging_legend_off
import com.breezone.app.resources.charging_legend_solar
import com.breezone.app.resources.charging_maximum
import com.breezone.app.resources.charging_next
import com.breezone.app.resources.charge_profiles_manage
import com.breezone.app.resources.charging_no_profile
import com.breezone.app.resources.charging_no_profile_admin
import com.breezone.app.resources.charging_none
import com.breezone.app.resources.charging_now
import com.breezone.app.resources.charging_now_hint
import com.breezone.app.resources.charging_now_kwh_left
import com.breezone.app.resources.charging_now_line
import com.breezone.app.resources.charging_now_shedding
import com.breezone.app.resources.charging_now_start
import com.breezone.app.resources.charging_now_until
import com.breezone.app.resources.charging_now_unplugged
import com.breezone.app.resources.charging_possible
import com.breezone.app.resources.charging_power
import com.breezone.app.resources.charging_profile
import com.breezone.app.resources.charging_profile_hint
import com.breezone.app.resources.charging_stop_now
import com.breezone.app.resources.charging_tomorrow_at
import com.breezone.app.resources.charging_until
import com.breezone.app.resources.charging_until_energy
import com.breezone.app.resources.charging_until_time
import com.breezone.app.resources.charging_until_unplugged
import com.breezone.app.resources.charging_up_to
import com.breezone.app.resources.days_names
import com.breezone.app.resources.tempo_blue
import com.breezone.app.resources.tempo_red
import com.breezone.app.resources.tempo_today
import com.breezone.app.resources.tempo_tomorrow
import com.breezone.app.resources.tempo_white
import com.breezone.app.ui.clock
import com.breezone.app.ui.decimalSeparator
import com.breezone.app.ui.icons.FzIcons
import com.breezone.app.ui.nowSeconds
import com.breezone.app.ui.theme.Fz
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.datetime.LocalDate
import kotlinx.datetime.TimeZone
import kotlinx.datetime.isoDayNumber
import kotlinx.datetime.toLocalDateTime
import org.jetbrains.compose.resources.stringArrayResource
import org.jetbrains.compose.resources.stringResource
import kotlin.math.roundToInt
import kotlin.time.Instant

/** What the charger follows and does, read every 10 s while shown. */
@Composable
internal fun rememberCharging(hub: Hub, serverId: String, deviceId: Long): MutableState<ChargingView?> {
    val view = remember(serverId, deviceId) { mutableStateOf<ChargingView?>(null) }
    LaunchedEffect(serverId, deviceId) {
        while (true) {
            runCatching { hub.api(serverId)?.charging(deviceId) }.getOrNull()?.let { view.value = it }
            delay(10_000)
        }
    }
    return view
}

/** "7,4" (one decimal, the separator of the language). */
private fun kw(value: Double): String = ((value * 10).roundToInt() / 10.0).toString().replace('.', decimalSeparator())

private fun date(ts: Long) = Instant.fromEpochSeconds(ts).toLocalDateTime(TimeZone.currentSystemDefault()).date

/** "22:00", "demain 06:00", "Mar 06:00". */
@Composable
private fun whenText(ts: Long): String {
    val days = date(ts).toEpochDays() - date(nowSeconds()).toEpochDays()
    val names = stringArrayResource(Res.array.days_names)
    return when (days) {
        0L -> clock(ts)
        1L -> stringResource(Res.string.charging_tomorrow_at, clock(ts))
        else -> "${names[date(ts).dayOfWeek.isoDayNumber - 1]} ${clock(ts)}"
    }
}

@Composable
private fun modeColor(mode: ChargeMode): Color = when (mode) {
    ChargeMode.Max -> MaterialTheme.colorScheme.primary
    ChargeMode.Solar -> Fz.colors.warning
    ChargeMode.Off -> MaterialTheme.colorScheme.surfaceVariant
}

@Composable
private fun tempoTone(color: DayColor): Color = when (color) {
    DayColor.Blue -> Fz.colors.cool
    DayColor.White -> MaterialTheme.colorScheme.onSurface
    DayColor.Red -> MaterialTheme.colorScheme.error
}

@Composable
private fun modeText(mode: ChargeMode, kw: Double?): String = when (mode) {
    ChargeMode.Max -> stringResource(Res.string.charging_up_to, kw(kw ?: 0.0))
    ChargeMode.Solar -> stringResource(Res.string.charging_legend_solar)
    ChargeMode.Off -> stringResource(Res.string.charging_legend_off)
}

@Composable
private fun NextCharge(next: ChargeNext) {
    val now = nowSeconds()
    val colorName = next.color?.let {
        stringResource(
            when (it) {
                DayColor.Blue -> Res.string.tempo_blue
                DayColor.White -> Res.string.tempo_white
                DayColor.Red -> Res.string.tempo_red
            },
        )
    }
    val hours = (next.end - maxOf(now, next.start)) / 3600.0
    val details = listOfNotNull(
        modeText(next.mode, next.kw),
        colorName?.let { stringResource(Res.string.charging_day_color, it.lowercase()) },
        next.kw?.takeIf { next.mode == ChargeMode.Max }?.let { stringResource(Res.string.charging_possible, formatEnergy(it * hours)) },
    )
    val end = if (date(next.start) == date(next.end)) clock(next.end) else whenText(next.end)
    Row(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(16.dp)).background(MaterialTheme.colorScheme.surfaceVariant).padding(12.dp),
        verticalAlignment = Alignment.Top,
    ) {
        Icon(FzIcons.Clock, null, Modifier.size(18.dp), tint = Fz.colors.muted)
        Spacer(Modifier.width(10.dp))
        Column {
            Text(
                if (next.start <= now) stringResource(Res.string.charging_in_progress, whenText(next.end))
                else stringResource(Res.string.charging_next, whenText(next.start), end),
                fontWeight = FontWeight.Medium,
            )
            Caption(details.joinToString(" · "))
        }
    }
}

/** Why the charger does what it does, the charge asked now, and when it charges next. */
@Composable
internal fun PlanCard(view: ChargingView, onCancelNow: () -> Unit) {
    val now = view.now
    val next = view.next.takeIf { now == null }
    // The regulator may not have seen yet a charge now called off.
    val reason = view.status?.takeIf { now == null && it.state != "now" }?.reason?.takeIf { it.isNotBlank() }
    if (now == null && next == null && reason == null) return
    SectionCard {
        if (now != null) {
            val end = when {
                now.until != null -> stringResource(Res.string.charging_until, whenText(now.until))
                now.leftKwh != null -> stringResource(Res.string.charging_now_kwh_left, formatEnergy(now.leftKwh))
                else -> stringResource(Res.string.charging_now_unplugged)
            }
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(FzIcons.Zap, null, Modifier.size(18.dp), tint = MaterialTheme.colorScheme.primary)
                Spacer(Modifier.width(8.dp))
                Text(stringResource(Res.string.charging_now_line, kw(now.kw ?: 0.0), end), fontWeight = FontWeight.Medium)
            }
            if (view.canControl) {
                OutlinedButton(onClick = onCancelNow, shape = Fz.shapes.button, modifier = Modifier.fillMaxWidth()) {
                    Text(stringResource(if (view.profileId != null) Res.string.charging_back_to_profile else Res.string.charging_stop_now))
                }
            }
        }
        reason?.let { Muted(it) }
        next?.let { NextCharge(it) }
    }
}

/** One local day of the profile, from midnight to midnight. */
@Composable
private fun DayStrip(day: ChargeDay, label: String) {
    val first = day.periods.firstOrNull()?.start ?: 0L
    val last = day.periods.lastOrNull()?.end ?: (first + 86_400)
    val span = (last - first).coerceAtLeast(1).toFloat()
    val now = nowSeconds()
    Row(verticalAlignment = Alignment.CenterVertically) {
        Row(Modifier.width(84.dp), verticalAlignment = Alignment.CenterVertically) {
            Caption(label, maxLines = 1)
            day.color?.let {
                Spacer(Modifier.width(4.dp))
                Box(Modifier.size(8.dp).background(tempoTone(it), CircleShape))
            }
        }
        Box(Modifier.weight(1f).height(20.dp).clip(RoundedCornerShape(6.dp)).background(MaterialTheme.colorScheme.surfaceVariant)) {
            Row(Modifier.fillMaxWidth().fillMaxHeight()) {
                day.periods.forEach { p ->
                    Box(
                        Modifier.weight(((p.end - p.start) / span).coerceAtLeast(0.001f)).fillMaxHeight().background(modeColor(p.mode)),
                        contentAlignment = Alignment.Center,
                    ) {
                        if (p.mode == ChargeMode.Max && p.end - p.start >= 4 * 3600) {
                            Text(stringResource(Res.string.charging_kw, kw(p.kw ?: 0.0)), fontSize = 10.sp, color = MaterialTheme.colorScheme.onPrimary, maxLines = 1)
                        }
                    }
                }
            }
            if (now in (first + 1) until last) {
                Row(Modifier.fillMaxWidth().fillMaxHeight()) {
                    Spacer(Modifier.weight(((now - first) / span).coerceAtLeast(0.001f)))
                    Box(Modifier.width(2.dp).fillMaxHeight().background(MaterialTheme.colorScheme.onSurface))
                    Spacer(Modifier.weight(((last - now) / span).coerceAtLeast(0.001f)))
                }
            }
        }
    }
}

/** The profiles the charger may follow, and the next days of the one it follows. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun ProfileCard(view: ChargingView, onManage: (() -> Unit)?, onChoose: (Long?) -> Unit) {
    val names = stringArrayResource(Res.array.days_names)
    SectionCard {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.weight(1f)) { SectionTitle(stringResource(Res.string.charging_profile)) }
            onManage?.let { TextButton(onClick = it) { Text(stringResource(Res.string.charge_profiles_manage)) } }
        }
        if (view.profiles.isEmpty()) {
            Muted(stringResource(if (onManage != null) Res.string.charging_no_profile_admin else Res.string.charging_no_profile))
            return@SectionCard
        }
        Caption(stringResource(if (view.profileId == null) Res.string.charging_any_profile else Res.string.charging_profile_hint))
        FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            view.profiles.forEach { p ->
                FilterChip(
                    selected = view.profileId == p.id,
                    enabled = view.canControl,
                    onClick = { if (view.profileId != p.id) onChoose(p.id) },
                    label = { Text(p.name, maxLines = 1, overflow = TextOverflow.Ellipsis) },
                )
            }
            FilterChip(
                selected = view.profileId == null,
                enabled = view.canControl,
                onClick = { if (view.profileId != null) onChoose(null) },
                label = { Text(stringResource(Res.string.charging_none)) },
            )
        }
        if (view.days.isNotEmpty()) {
            Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                view.days.forEachIndexed { i, d ->
                    val label = when (i) {
                        0 -> stringResource(Res.string.tempo_today)
                        1 -> stringResource(Res.string.tempo_tomorrow)
                        else -> names[LocalDate.parse(d.day).dayOfWeek.isoDayNumber - 1]
                    }
                    DayStrip(d, label)
                }
                Row(Modifier.padding(start = 84.dp)) {
                    listOf("0 h", "6 h", "12 h", "18 h", "24 h").forEachIndexed { i, h ->
                        if (i > 0) Spacer(Modifier.weight(1f))
                        Text(h, fontSize = 10.sp, color = Fz.colors.muted)
                    }
                }
            }
            FlowRow(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                listOf(ChargeMode.Max to Res.string.charging_legend_max, ChargeMode.Solar to Res.string.charging_legend_solar, ChargeMode.Off to Res.string.charging_legend_off)
                    .forEach { (m, text) ->
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Box(Modifier.size(10.dp).clip(RoundedCornerShape(3.dp)).background(modeColor(m)))
                            Spacer(Modifier.width(5.dp))
                            Caption(stringResource(text))
                        }
                    }
            }
        }
    }
}

private enum class Until { Unplugged, Energy, Time }

/** The next time "HH:mm" comes (today or tomorrow). */
private fun nextTime(hhmm: String): Long? {
    val (h, m) = hhmm.split(":").mapNotNull { it.toIntOrNull() }.takeIf { it.size == 2 } ?: return null
    if (h !in 0..23 || m !in 0..59) return null
    val now = nowSeconds()
    val local = Instant.fromEpochSeconds(now).toLocalDateTime(TimeZone.currentSystemDefault())
    val ahead = (h * 3600 + m * 60) - (local.hour * 3600 + local.minute * 60 + local.second)
    return now + ahead + if (ahead <= 0) 86_400 else 0
}

/** "Charge now": at some kW, until unplugged, some kWh more or a time. */
@OptIn(ExperimentalMaterial3Api::class, ExperimentalLayoutApi::class)
@Composable
internal fun ChargeNowSheet(charger: DeviceView, onDismiss: () -> Unit, onStart: (ChargeNowInput) -> Unit) {
    val s = charger.stateAs(EvChargerState.serializer()) ?: EvChargerState()
    val volts = charger.state?.get("voltage")?.toString()?.toDoubleOrNull()?.takeIf { it > 0 } ?: 230.0
    val phases = (charger.state?.get("phases")?.toString()?.toDoubleOrNull() ?: 1.0).coerceAtLeast(1.0)
    val lo = ((s.limitMin ?: 6) * volts * phases / 1000).toFloat()
    val hi = ((s.limitMax ?: 32) * volts * phases / 1000).toFloat().coerceAtLeast(lo + 0.1f)
    var power by remember { mutableFloatStateOf(hi) }
    var until by remember { mutableStateOf(Until.Unplugged) }
    var energy by remember { mutableStateOf("10") }
    var at by remember { mutableStateOf("07:00") }
    val atMax = power >= hi - 0.05f
    val amount = energy.replace(',', '.').toDoubleOrNull()
    val valid = when (until) {
        Until.Unplugged -> true
        Until.Energy -> amount != null && amount > 0 && amount <= 200
        Until.Time -> nextTime(at) != null
    }
    val sheet = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    val scope = rememberCoroutineScope()
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = sheet) {
        Column(
            Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).padding(horizontal = 20.dp).navigationBarsPadding(),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text(stringResource(Res.string.charging_now), style = MaterialTheme.typography.titleLarge)
            Caption(stringResource(Res.string.charging_now_hint))
            Caption(stringResource(Res.string.charging_power))
            Text(
                if (atMax) stringResource(Res.string.charging_kw_max, kw(hi.toDouble())) else stringResource(Res.string.charging_kw, kw(power.toDouble())),
                fontSize = 28.sp,
                fontWeight = FontWeight.SemiBold,
            )
            val slider = remember(lo, hi) { SliderState(value = power, trackRange = lo..hi) }
            LaunchedEffect(power) { if (kotlin.math.abs(slider.value - power) > 0.01f) slider.value = power }
            Slider(state = slider, onValueChange = { slider.value = it; power = (it * 10).roundToInt() / 10f })
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                listOf(3.7f, 7.4f).filter { it > lo && it < hi - 0.05f }.forEach { p ->
                    FilterChip(selected = kotlin.math.abs(power - p) < 0.05f, onClick = { power = p }, label = { Text(stringResource(Res.string.charging_kw, kw(p.toDouble()))) })
                }
                FilterChip(selected = atMax, onClick = { power = hi }, label = { Text(stringResource(Res.string.charging_maximum)) })
            }
            Caption(stringResource(Res.string.charging_now_until))
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                listOf(Until.Unplugged to Res.string.charging_until_unplugged, Until.Energy to Res.string.charging_until_energy, Until.Time to Res.string.charging_until_time)
                    .forEach { (u, text) -> FilterChip(selected = until == u, onClick = { until = u }, label = { Text(stringResource(text)) }) }
            }
            when (until) {
                Until.Energy -> OutlinedTextField(
                    value = energy,
                    onValueChange = { energy = it },
                    suffix = { Text(stringResource(Res.string.charging_kwh_unit)) },
                    singleLine = true,
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
                )
                Until.Time -> OutlinedTextField(
                    value = at,
                    onValueChange = { at = it },
                    singleLine = true,
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                )
                Until.Unplugged -> {}
            }
            if (s.autoLimit == true) Caption(stringResource(Res.string.charging_now_shedding))
            Button(
                onClick = {
                    val input = ChargeNowInput(
                        kw = if (atMax) null else power.toDouble(),
                        kwh = amount.takeIf { until == Until.Energy },
                        until = if (until == Until.Time) nextTime(at) else null,
                    )
                    scope.launch { sheet.hide() }.invokeOnCompletion { onStart(input) }
                },
                enabled = valid,
                shape = Fz.shapes.button,
                modifier = Modifier.fillMaxWidth().padding(bottom = 16.dp),
            ) {
                Icon(FzIcons.Zap, null, Modifier.size(16.dp))
                Spacer(Modifier.width(6.dp))
                Text(stringResource(Res.string.charging_now_start, kw(if (atMax) hi.toDouble() else power.toDouble())))
            }
        }
    }
}
