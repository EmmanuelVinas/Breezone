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
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.breezone.app.data.ChargeDay
import com.breezone.app.data.ChargeEffect
import com.breezone.app.data.ChargeEffectMode
import com.breezone.app.data.ChargeMode
import com.breezone.app.data.ChargeProfileRef
import com.breezone.app.data.ChargeNext
import com.breezone.app.data.ChargeNowInput
import com.breezone.app.data.ChargingView
import com.breezone.app.data.DayColor
import com.breezone.app.data.DeviceView
import com.breezone.app.data.EvChargerState
import com.breezone.app.data.Hub
import com.breezone.app.resources.*
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

/** "#14b8a6" → a colour (none: not one). */
internal fun hexColor(hex: String?): Color? = hex?.removePrefix("#")?.takeIf { it.length == 6 }?.toLongOrNull(16)?.let { Color(0xFF000000 or it) }

/** The colour of a stretch of a frieze: a profile by its own, the rest by what it is. */
@Composable
internal fun effectColor(effect: ChargeEffect?, profiles: List<ChargeProfileRef>): Color = when (effect?.mode) {
    ChargeEffectMode.Profile -> hexColor(profiles.firstOrNull { it.id == effect.profileId }?.color) ?: MaterialTheme.colorScheme.primary
    ChargeEffectMode.Now -> MaterialTheme.colorScheme.primary
    ChargeEffectMode.Solar -> Fz.colors.warning
    ChargeEffectMode.Off -> MaterialTheme.colorScheme.surfaceVariant
    ChargeEffectMode.Free, null -> Color.Transparent
}

/** What a stretch of a frieze tells the charger, in a few words. */
@Composable
internal fun effectText(effect: ChargeEffect?, profiles: List<ChargeProfileRef>): String = when (effect?.mode) {
    ChargeEffectMode.Profile -> profiles.firstOrNull { it.id == effect.profileId }?.name ?: stringResource(Res.string.charge_profiles_deleted)
    ChargeEffectMode.Now -> stringResource(Res.string.charging_now)
    ChargeEffectMode.Solar -> stringResource(Res.string.charging_legend_solar)
    ChargeEffectMode.Off -> stringResource(Res.string.charging_legend_off)
    ChargeEffectMode.Free -> stringResource(Res.string.charging_free)
    null -> ""
}

internal fun dim(effect: ChargeEffect?) = effect == null || effect.mode == ChargeEffectMode.Off || effect.mode == ChargeEffectMode.Free

@Composable
private fun tempoTone(color: DayColor): Color = when (color) {
    DayColor.Blue -> Fz.colors.cool
    DayColor.White -> MaterialTheme.colorScheme.onSurface
    DayColor.Red -> MaterialTheme.colorScheme.error
}

@Composable
private fun NextCharge(next: ChargeNext) {
    val now = nowSeconds()
    val hours = (next.end - maxOf(now, next.start)) / 3600.0
    val what = when (next.effect.mode) {
        ChargeEffectMode.Solar -> stringResource(Res.string.charging_legend_solar)
        else -> stringResource(Res.string.charging_up_to, kw(next.kw ?: 0.0))
    }
    val details = listOfNotNull(
        what,
        next.automation?.name,
        next.kw?.takeIf { next.effect.mode != ChargeEffectMode.Solar }?.let { stringResource(Res.string.charging_possible, formatEnergy(it * hours)) },
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
    val next = view.next.takeIf { now == null && !view.ownSchedule }
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
                    Text(stringResource(if (view.automations.isNotEmpty() && !view.ownSchedule) Res.string.charging_back_to_profile else Res.string.charging_stop_now))
                }
            }
        }
        reason?.let { Muted(it) }
        next?.let { NextCharge(it) }
    }
}

/** One local day of what the automations plan, from midnight to midnight. */
@Composable
private fun DayStrip(day: ChargeDay, label: String, profiles: List<ChargeProfileRef>) {
    val first = day.segments.firstOrNull()?.start ?: 0L
    val last = day.segments.lastOrNull()?.end ?: (first + 86_400)
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
        Box(Modifier.weight(1f).height(20.dp).clip(RoundedCornerShape(6.dp)).background(MaterialTheme.colorScheme.background)) {
            Row(Modifier.fillMaxWidth().fillMaxHeight()) {
                day.segments.forEach { p ->
                    Box(
                        Modifier.weight(((p.end - p.start) / span).coerceAtLeast(0.001f)).fillMaxHeight().background(effectColor(p.effect, profiles)),
                        contentAlignment = Alignment.Center,
                    ) {
                        if (!dim(p.effect) && p.end - p.start >= 4 * 3600) {
                            Text(effectText(p.effect, profiles), fontSize = 10.sp, color = MaterialTheme.colorScheme.onPrimary, maxLines = 1, overflow = TextOverflow.Clip)
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

/** The legend of the friezes: each profile, the surplus, no charge. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun EffectLegend(profiles: List<ChargeProfileRef>) {
    FlowRow(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        val items = profiles.map { it.name to (hexColor(it.color) ?: MaterialTheme.colorScheme.primary) } +
            listOf(
                stringResource(Res.string.charging_legend_solar) to Fz.colors.warning,
                stringResource(Res.string.charging_legend_off) to MaterialTheme.colorScheme.surfaceVariant,
            )
        items.forEach { (text, color) ->
            Row(verticalAlignment = Alignment.CenterVertically) {
                Box(Modifier.size(10.dp).clip(RoundedCornerShape(3.dp)).background(color))
                Spacer(Modifier.width(5.dp))
                Caption(text)
            }
        }
    }
}

/** What the automations tell the charger now and over the next days (read only). */
@Composable
internal fun ProfileCard(view: ChargingView, onManage: (() -> Unit)?) {
    val names = stringArrayResource(Res.array.days_names)
    SectionCard {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.weight(1f)) { SectionTitle(stringResource(Res.string.charging_plan_title)) }
            onManage?.let { TextButton(onClick = it) { Text(stringResource(Res.string.charge_profiles_manage)) } }
        }
        val current = when (view.mode) {
            ChargeMode.Profile -> view.profiles.firstOrNull { it.id == view.profileId }?.name
            ChargeMode.Off -> stringResource(Res.string.charging_legend_off)
            ChargeMode.Solar -> stringResource(Res.string.charging_legend_solar)
            ChargeMode.Free -> null
        }
        if (view.ownSchedule) {
            Column(
                Modifier.fillMaxWidth().clip(RoundedCornerShape(16.dp)).background(MaterialTheme.colorScheme.surfaceVariant).padding(12.dp),
            ) {
                Text(stringResource(Res.string.charging_app_decides), fontWeight = FontWeight.Medium)
                Caption(
                    if (view.automations.isEmpty()) stringResource(Res.string.charging_app_decides_hint)
                    else stringResource(Res.string.charging_app_decides_rules, view.automations.joinToString(", ") { it.name }),
                )
            }
        } else {
            Caption(
                when {
                    current != null && view.automation != null -> stringResource(Res.string.charging_plan_now_by, current, view.automation.name)
                    current != null -> stringResource(Res.string.charging_plan_now, current)
                    view.automations.isNotEmpty() -> stringResource(Res.string.charging_plan_waiting)
                    else -> stringResource(Res.string.charging_plan_none)
                },
            )
        }
        if (view.days.isNotEmpty() && view.automations.isNotEmpty()) {
            Column(Modifier.alpha(if (view.ownSchedule) 0.4f else 1f), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                view.days.forEachIndexed { i, d ->
                    val label = when (i) {
                        0 -> stringResource(Res.string.tempo_today)
                        1 -> stringResource(Res.string.tempo_tomorrow)
                        else -> names[LocalDate.parse(d.day).dayOfWeek.isoDayNumber - 1]
                    }
                    DayStrip(d, label, view.profiles)
                }
                Row(Modifier.padding(start = 84.dp)) {
                    listOf("0 h", "6 h", "12 h", "18 h", "24 h").forEachIndexed { i, h ->
                        if (i > 0) Spacer(Modifier.weight(1f))
                        Text(h, fontSize = 10.sp, color = Fz.colors.muted)
                    }
                }
                EffectLegend(view.profiles)
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
