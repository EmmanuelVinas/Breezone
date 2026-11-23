package com.breezone.app.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Button
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
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
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.breezone.app.data.ChargeCalendar
import com.breezone.app.data.ChargeColors
import com.breezone.app.data.ChargeDayKind
import com.breezone.app.data.ChargeMode
import com.breezone.app.data.ChargeProfileView
import com.breezone.app.data.ChargeProfileWrite
import com.breezone.app.data.ChargeSlot
import com.breezone.app.data.Hub
import com.breezone.app.resources.*
import com.breezone.app.ui.decimalSeparator
import com.breezone.app.ui.icons.FzIcons
import com.breezone.app.ui.theme.Fz
import kotlinx.coroutines.launch
import org.jetbrains.compose.resources.getString
import org.jetbrains.compose.resources.stringArrayResource
import org.jetbrains.compose.resources.stringResource
import org.koin.compose.koinInject
import kotlin.math.roundToInt

private const val DAY = 24 * 60
private const val STEP = 30

private fun minutes(hhmm: String): Int? {
    val parts = hhmm.split(":")
    if (parts.size != 2 || parts[0].length != 2 || parts[1].length != 2) return null
    val (h, m) = parts.map { it.toIntOrNull() ?: return null }
    return if (h in 0..23 && m in 0..59) h * 60 + m else null
}

private fun clockOf(min: Int) = "${(min / 60).toString().padStart(2, '0')}:${(min % 60).toString().padStart(2, '0')}"

private fun slotEnd(slots: List<ChargeSlot>, i: Int) = slots.getOrNull(i + 1)?.let { minutes(it.start) } ?: DAY

/** The slot cut in two at its middle (on a half hour), when it is long enough. */
private fun split(slots: List<ChargeSlot>, i: Int): List<ChargeSlot>? {
    val start = minutes(slots[i].start) ?: return null
    val end = slotEnd(slots, i)
    if (end - start < 2 * STEP) return null
    val middle = start + ((end - start) / 2.0 / STEP).roundToInt() * STEP
    return slots.take(i + 1) + slots[i].copy(start = clockOf(middle)) + slots.drop(i + 1)
}

/** The slot given to the one before it (the first one to the next). */
private fun merge(slots: List<ChargeSlot>, i: Int): List<ChargeSlot> = when {
    slots.size < 2 -> slots
    i == 0 -> listOf(slots[1].copy(start = "00:00")) + slots.drop(2)
    else -> slots.filterIndexed { j, _ -> j != i }
}

/** The start of slot `i` (not the first) moved to `to`, between its neighbours. */
private fun moveStart(slots: List<ChargeSlot>, i: Int, to: Int): List<ChargeSlot> {
    val low = (minutes(slots[i - 1].start) ?: 0) + STEP
    val high = slotEnd(slots, i) - STEP
    val at = to.coerceIn(low, maxOf(low, high))
    return slots.mapIndexed { j, s -> if (j == i) s.copy(start = clockOf(at)) else s }
}

private val OFF = ChargeSlot("00:00", ChargeMode.Off)
private val NIGHT = listOf(ChargeSlot("00:00", ChargeMode.Max, kw = 7.0), ChargeSlot("06:00", ChargeMode.Off), ChargeSlot("22:00", ChargeMode.Max, kw = 7.0))
private const val ORANGE = "#f59e0b"
private const val TEAL = "#14b8a6"
private const val BLUE = "#3b82f6"
private const val RED = "#ef4444"

/** Profiles to start from: off-peak hours, blue nights (Tempo), solar. */
private suspend fun templates(): List<ChargeProfileWrite> {
    val every = { id: String -> List(7) { id } }
    val all = getString(Res.string.charge_profiles_every_day)
    return listOf(
        ChargeProfileWrite(getString(Res.string.charge_profiles_offpeak), listOf(ChargeDayKind("d", all, TEAL, NIGHT)), ChargeCalendar("00:00", every("d"))),
        ChargeProfileWrite(
            getString(Res.string.charge_profiles_blue_night),
            listOf(
                ChargeDayKind("blue", getString(Res.string.charge_profiles_blue_day), BLUE, NIGHT),
                ChargeDayKind("other", getString(Res.string.charge_profiles_white_red), RED, listOf(OFF)),
            ),
            ChargeCalendar("06:00", every("blue"), ChargeColors(blue = "blue", white = "other", red = "other")),
        ),
        ChargeProfileWrite(
            getString(Res.string.charge_profiles_solar),
            listOf(ChargeDayKind("d", all, ORANGE, listOf(ChargeSlot("00:00", ChargeMode.Solar)))),
            ChargeCalendar("00:00", every("d")),
        ),
    )
}

/** What is wrong with a profile, as the server would say it (null: nothing). */
@Composable
private fun problem(p: ChargeProfileWrite): String? = when {
    p.name.isBlank() -> stringResource(Res.string.charge_profiles_needs_name)
    p.profiles.any { it.name.isBlank() } -> stringResource(Res.string.charge_profiles_kind_needs_name)
    p.profiles.any { k -> k.slots.any { s -> s.kw != null && (s.kw <= 0 || s.kw > 50) } } -> stringResource(Res.string.charge_profiles_power_range)
    else -> null
}

private fun parseColor(hex: String?): Color? = hex?.removePrefix("#")?.toLongOrNull(16)?.let { Color(0xFF000000 or it) }

@Composable
private fun modeColor(mode: ChargeMode): Color = when (mode) {
    ChargeMode.Max -> MaterialTheme.colorScheme.primary
    ChargeMode.Solar -> Fz.colors.warning
    ChargeMode.Off -> MaterialTheme.colorScheme.surfaceVariant
}

/** Recharge › Profiles: the charging profiles of the home (for its administrators). */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun ChargeProfilesScreen(serverId: String, onBack: () -> Unit) {
    val hub = koinInject<Hub>()
    val scope = rememberCoroutineScope()
    var list by remember { mutableStateOf<List<ChargeProfileView>?>(null) }
    var models by remember { mutableStateOf<List<ChargeProfileWrite>>(emptyList()) }
    /** The profile shown (`null` with a draft: a new one). */
    var savedId by remember { mutableStateOf<Long?>(null) }
    var draft by remember { mutableStateOf<ChargeProfileWrite?>(null) }
    var initial by remember { mutableStateOf<ChargeProfileWrite?>(null) }
    var confirmDelete by remember { mutableStateOf(false) }
    val open = { p: ChargeProfileView ->
        val write = ChargeProfileWrite(p.name, p.profiles, p.calendar)
        savedId = p.id
        draft = write
        initial = write
    }
    val reload: suspend (Long?) -> Unit = { show ->
        val read = hub.attempt(serverId) { chargeProfiles() }
        if (read != null) {
            list = read
            read.firstOrNull { it.id == show }?.let(open) ?: read.firstOrNull()?.takeIf { draft == null }?.let(open)
        }
    }
    LaunchedEffect(serverId) {
        models = templates()
        reload(null)
    }
    val saved = list?.firstOrNull { it.id == savedId }
    Scaffold(topBar = { FzTopBar(stringResource(Res.string.charge_profiles_title), stringResource(Res.string.house_charging), onBack) }) { padding ->
        LazyColumn(
            Modifier.fillMaxSize(),
            contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = padding.calculateTopPadding() + 4.dp, bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            item(key = "choose", contentType = "chips") {
                SectionCard {
                    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        list.orEmpty().forEach { p ->
                            FilterChip(selected = savedId == p.id && draft != null, onClick = { open(p) }, label = { Text(p.name) })
                        }
                    }
                    Caption(stringResource(Res.string.charge_profiles_new))
                    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        models.forEach { m ->
                            OutlinedButton(onClick = {
                                savedId = null
                                draft = m
                                initial = null
                            }, shape = Fz.shapes.button) { Text(m.name) }
                        }
                    }
                }
            }
            val d = draft
            if (d == null) {
                item(key = "none", contentType = "note") { SectionCard { Muted(stringResource(Res.string.charge_profiles_none)) } }
                return@LazyColumn
            }
            item(key = "head-${savedId ?: "new"}", contentType = "head") {
                SectionCard {
                    OutlinedTextField(
                        value = d.name,
                        onValueChange = { draft = d.copy(name = it) },
                        label = { Text(stringResource(Res.string.charge_profiles_name)) },
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth(),
                    )
                    val issue = problem(d)
                    Caption(issue ?: stringResource(Res.string.charge_profiles_hint), color = if (issue != null) MaterialTheme.colorScheme.error else Fz.colors.muted)
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                        Button(
                            onClick = {
                                scope.launch {
                                    val id = savedId
                                    val done = hub.attempt(serverId) { if (id != null) saveChargeProfile(id, d) else createChargeProfile(d) }
                                    if (done != null) {
                                        savedId = done.id
                                        reload(done.id)
                                    }
                                }
                            },
                            enabled = issue == null && d != initial,
                            shape = Fz.shapes.button,
                        ) { Text(stringResource(Res.string.common_save)) }
                        if (savedId != null) IconTextButton(FzIcons.Trash2, stringResource(Res.string.common_delete), { confirmDelete = true }, MaterialTheme.colorScheme.error)
                    }
                }
            }
            itemsIndexed(d.profiles, key = { _, k -> "kind-${k.id}" }, contentType = { _, _ -> "kind" }) { i, k ->
                KindCard(
                    kind = k,
                    onChange = { next -> draft = d.copy(profiles = d.profiles.mapIndexed { j, o -> if (j == i) next else o }) },
                    onRemove = if (d.profiles.size > 1) ({
                        val rest = d.profiles.filterIndexed { j, _ -> j != i }
                        val swap = { id: String? -> if (id == k.id) rest.first().id else id }
                        val c = d.calendar
                        draft = d.copy(
                            profiles = rest,
                            calendar = c.copy(weekdays = c.weekdays.map { swap(it)!! }, colors = ChargeColors(swap(c.colors.blue), swap(c.colors.white), swap(c.colors.red))),
                        )
                    }) else null,
                )
            }
            item(key = "add-kind", contentType = "add") {
                OutlinedButton(
                    onClick = {
                        var n = d.profiles.size + 1
                        while (d.profiles.any { it.id == "p$n" }) n++
                        scope.launch {
                            val name = getString(Res.string.charge_profiles_kind_n, d.profiles.size + 1)
                            draft = d.copy(profiles = d.profiles + ChargeDayKind("p$n", name, TEAL, listOf(OFF)))
                        }
                    },
                    enabled = d.profiles.size < 10,
                    shape = Fz.shapes.button,
                    modifier = Modifier.fillMaxWidth(),
                ) { Text(stringResource(Res.string.charge_profiles_add_kind)) }
            }
            item(key = "calendar", contentType = "calendar") { CalendarCard(d) { draft = d.copy(calendar = it) } }
            if (saved != null && (saved.usedBy.isNotEmpty() || saved.automations.isNotEmpty())) {
                item(key = "used", contentType = "used") {
                    SectionCard {
                        SectionTitle(stringResource(Res.string.charge_profiles_used_by))
                        saved.usedBy.forEach { Text(it.name) }
                        saved.automations.forEach { Text(stringResource(Res.string.charge_profiles_automation, it)) }
                    }
                }
            }
        }
    }
    if (confirmDelete) {
        ConfirmDialog(
            title = stringResource(Res.string.charge_profiles_delete_confirm, draft?.name.orEmpty()),
            confirm = stringResource(Res.string.common_delete),
            onConfirm = {
                confirmDelete = false
                val id = savedId ?: return@ConfirmDialog
                scope.launch {
                    if (hub.attempt(serverId) { deleteChargeProfile(id) } != null) {
                        savedId = null
                        draft = null
                        initial = null
                        reload(null)
                    }
                }
            },
            onDismiss = { confirmDelete = false },
        )
    }
}

private fun kwText(kw: Double?) = kw?.let { ((it * 10).roundToInt() / 10.0).toString().removeSuffix(".0").replace('.', decimalSeparator()) }.orEmpty()

/** A kind of day: its name, its day as a bar, and its slots, one per line. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun KindCard(kind: ChargeDayKind, onChange: (ChargeDayKind) -> Unit, onRemove: (() -> Unit)?) {
    val slots = kind.slots
    val setSlots = { next: List<ChargeSlot> -> onChange(kind.copy(slots = next)) }
    SectionCard {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.width(12.dp).height(12.dp).clip(RoundedCornerShape(6.dp)).background(parseColor(kind.color) ?: Fz.colors.muted))
            Spacer(Modifier.width(10.dp))
            OutlinedTextField(
                value = kind.name,
                onValueChange = { onChange(kind.copy(name = it)) },
                singleLine = true,
                label = { Text(stringResource(Res.string.charge_profiles_kind_name)) },
                modifier = Modifier.weight(1f),
            )
            if (onRemove != null) IconTextButton(FzIcons.Trash2, stringResource(Res.string.common_delete), onRemove, MaterialTheme.colorScheme.error)
        }
        Row(Modifier.fillMaxWidth().height(18.dp).clip(RoundedCornerShape(6.dp)).background(MaterialTheme.colorScheme.surfaceVariant)) {
            slots.forEachIndexed { i, s ->
                val span = (slotEnd(slots, i) - (minutes(s.start) ?: 0)).coerceAtLeast(1)
                Box(Modifier.weight(span.toFloat()).fillMaxHeight().background(modeColor(s.mode)))
            }
        }
        slots.forEachIndexed { i, s ->
            Column(
                Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp)).background(MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.5f)).padding(10.dp),
                verticalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    if (i == 0) {
                        Text(clockOf(0), fontWeight = FontWeight.Medium)
                    } else {
                        TimeField(s.start) { m -> setSlots(moveStart(slots, i, m)) }
                    }
                    Text(stringResource(Res.string.charge_profiles_until, clockOf(slotEnd(slots, i))), color = Fz.colors.muted)
                    Spacer(Modifier.weight(1f))
                    split(slots, i)?.let { cut ->
                        IconButton(onClick = { setSlots(cut) }) { Icon(FzIcons.Scissors, stringResource(Res.string.charge_profiles_cut), Modifier.width(18.dp).height(18.dp)) }
                    }
                    if (slots.size > 1) {
                        IconButton(onClick = { setSlots(merge(slots, i)) }) { Icon(FzIcons.Merge, stringResource(Res.string.charge_profiles_merge), Modifier.width(18.dp).height(18.dp)) }
                    }
                }
                FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    listOf(ChargeMode.Max to Res.string.charge_profiles_mode_max, ChargeMode.Solar to Res.string.charge_profiles_mode_solar, ChargeMode.Off to Res.string.charge_profiles_mode_off)
                        .forEach { (m, text) ->
                            FilterChip(
                                selected = s.mode == m,
                                onClick = { setSlots(slots.mapIndexed { j, o -> if (j == i) ChargeSlot(o.start, m, kw = if (m == ChargeMode.Max) o.kw ?: 7.0 else null) else o }) },
                                label = { Text(stringResource(text), fontSize = 13.sp) },
                            )
                        }
                }
                if (s.mode == ChargeMode.Max) {
                    var text by remember(i, kind.id) { mutableStateOf(kwText(s.kw)) }
                    OutlinedTextField(
                        value = text,
                        onValueChange = { v ->
                            text = v
                            val kw = v.replace(',', '.').toDoubleOrNull()
                            if (v.isBlank() || (kw != null && kw > 0)) setSlots(slots.mapIndexed { j, o -> if (j == i) o.copy(kw = kw) else o })
                        },
                        singleLine = true,
                        label = { Text(stringResource(Res.string.charge_profiles_up_to_kw)) },
                        placeholder = { Text(stringResource(Res.string.charging_maximum)) },
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
                        modifier = Modifier.width(160.dp),
                    )
                }
            }
        }
    }
}

/** "HH:MM", given back once it is one. */
@Composable
private fun TimeField(value: String, onChange: (Int) -> Unit) {
    var text by remember(value) { mutableStateOf(value) }
    OutlinedTextField(
        value = text,
        onValueChange = { v ->
            text = v
            minutes(v)?.let(onChange)
        },
        singleLine = true,
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
        modifier = Modifier.width(96.dp),
    )
}

/** Which kind of day a day follows: by the day of the week, or by the Tempo colour. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun CalendarCard(d: ChargeProfileWrite, onChange: (ChargeCalendar) -> Unit) {
    val c = d.calendar
    val names = stringArrayResource(Res.array.days_names)
    val choose = @Composable { current: String?, set: (String) -> Unit ->
        FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            d.profiles.forEach { k -> FilterChip(selected = current == k.id, onClick = { set(k.id) }, label = { Text(k.name, fontSize = 13.sp) }) }
        }
    }
    SectionCard {
        SectionTitle(stringResource(Res.string.charge_profiles_calendar))
        Text(stringResource(Res.string.charge_profiles_day_start))
        FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            listOf("00:00", "06:00").forEach { t -> FilterChip(selected = c.dayStart == t, onClick = { onChange(c.copy(dayStart = t)) }, label = { Text(t) }) }
        }
        Caption(stringResource(Res.string.charge_profiles_day_start_hint))
        // With a single kind of day, every day follows it.
        if (d.profiles.size > 1) {
            names.forEachIndexed { i, day ->
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(day, Modifier.width(48.dp), color = Fz.colors.muted)
                    choose(c.weekdays.getOrNull(i)) { id -> onChange(c.copy(weekdays = List(7) { j -> if (j == i) id else c.weekdays.getOrElse(j) { id } })) }
                }
            }
        }
        Row(verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(stringResource(Res.string.charge_profiles_by_color))
                Caption(stringResource(Res.string.charge_profiles_by_color_hint))
            }
            Switch(checked = c.colors.any, onCheckedChange = { on ->
                val ids = d.profiles.map { it.id }
                onChange(c.copy(colors = if (on) ChargeColors(ids.first(), ids.getOrElse(1) { ids.first() }, ids.last()) else ChargeColors()))
            })
        }
        if (c.colors.any) {
            listOf(
                Triple(Res.string.tempo_blue, c.colors.blue) { id: String -> c.copy(colors = c.colors.copy(blue = id)) },
                Triple(Res.string.tempo_white, c.colors.white) { id: String -> c.copy(colors = c.colors.copy(white = id)) },
                Triple(Res.string.tempo_red, c.colors.red) { id: String -> c.copy(colors = c.colors.copy(red = id)) },
            ).forEach { (label, current, set) ->
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(stringResource(label), Modifier.width(64.dp), color = Fz.colors.muted)
                    choose(current) { onChange(set(it)) }
                }
            }
        }
    }
}
