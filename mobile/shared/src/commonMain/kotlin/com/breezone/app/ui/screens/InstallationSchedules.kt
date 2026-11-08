package com.breezone.app.ui.screens

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TimePicker
import androidx.compose.material3.rememberTimePickerState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.breezone.app.data.AirzoneSchedule
import com.breezone.app.data.AirzoneScheduleInput
import com.breezone.app.data.Hub
import com.breezone.app.data.Installation
import com.breezone.app.data.Mode
import com.breezone.app.data.ScheduleZone
import com.breezone.app.data.SchedulesState
import com.breezone.app.data.Zone
import com.breezone.app.data.ZoneCommand
import com.breezone.app.data.zone
import com.breezone.app.resources.*
import com.breezone.app.ui.SetpointLimits
import com.breezone.app.ui.formatTemp
import com.breezone.app.ui.icons.FzIcons
import com.breezone.app.ui.label
import com.breezone.app.ui.scheduleText
import com.breezone.app.ui.theme.Fz
import kotlinx.coroutines.launch
import org.jetbrains.compose.resources.getString
import org.jetbrains.compose.resources.stringArrayResource
import org.jetbrains.compose.resources.stringResource
import org.koin.compose.koinInject

/** Airzone limits schedule names to 11 characters. */
private const val NAME_MAX = 11

/** Schedules of the Airzone account for one installation, shown in its "Schedules" tab. */
@Composable
fun InstallationSchedules(serverId: String, inst: Installation, modifier: Modifier = Modifier) {
    val hub = koinInject<Hub>()
    val scope = rememberCoroutineScope()
    var state by remember { mutableStateOf<SchedulesState?>(null) }
    var cloudId by remember { mutableStateOf<String?>(null) }
    var editing by remember { mutableStateOf<AirzoneSchedule?>(null) }
    var creating by remember { mutableStateOf(false) }
    var refreshKey by remember { mutableIntStateOf(0) }

    LaunchedEffect(serverId, refreshKey) {
        val api = hub.api(serverId) ?: return@LaunchedEffect
        hub.attempt { api.schedules() }?.let { state = it }
        // The Airzone installation behind this one: needed to create a schedule.
        cloudId = state?.schedules?.firstOrNull { it.installationId == inst.id }?.cloudId
            ?: runCatching { api.cloud() }.getOrNull()?.installations?.firstOrNull { it.installationId == inst.id }?.cloudId
    }

    /** Runs a call that answers with the new schedules. */
    fun apply(call: suspend () -> SchedulesState?) {
        hub.launchAttempt(scope) { call()?.let { state = it } }
    }

    val s = state
    val schedules = s?.schedules.orEmpty().filter { it.installationId == inst.id }
    SectionList(
        hint = stringResource(Res.string.schedules_hint),
        addLabel = stringResource(Res.string.schedules_new),
        onAdd = if (inst.controllable && s?.account == true && cloudId != null) ({ creating = true }) else null,
        modifier = modifier,
    ) {
        when {
            s == null -> Unit
            !s.account -> item(key = "no-account", contentType = "message") { Muted(stringResource(Res.string.schedules_no_account)) }
            else -> {
                s.lastError?.let { error ->
                    item(key = "error", contentType = "message") {
                        Text(stringResource(Res.string.schedules_last_error, error), fontSize = 13.sp, color = MaterialTheme.colorScheme.error)
                    }
                }
                if (schedules.isEmpty()) {
                    item(key = "empty", contentType = "message") { Muted(stringResource(Res.string.schedules_empty)) }
                }
            }
        }
        items(schedules, key = { "${it.cloudId}-${it.id}" }, contentType = { "schedule" }) { schedule ->
            val canEdit = schedule.editable && inst.controllable
            ScheduleCard(
                schedule = schedule,
                inst = inst,
                canEdit = canEdit,
                onClick = { if (canEdit) editing = schedule },
                onEnabled = { enabled -> apply { hub.api(serverId)?.setScheduleEnabled(schedule, enabled) } },
            )
        }
    }

    if (creating || editing != null) {
        ScheduleEditor(
            inst = inst,
            schedule = editing,
            onDismiss = {
                creating = false
                editing = null
            },
            onSave = { input ->
                val target = editing
                val api = hub.api(serverId) ?: error(getString(Res.string.error_server_unavailable))
                state = if (target == null) api.createSchedule(input.copy(cloudId = cloudId)) else api.updateSchedule(target, input)
                hub.notify(getString(Res.string.schedules_saved))
                creating = false
                editing = null
            },
            onDelete = { target ->
                apply { hub.api(serverId)?.deleteSchedule(target) }
                editing = null
            },
        )
    }
}

@Composable
private fun actionText(a: ZoneCommand): String {
    if (a.on == false) return stringResource(Res.string.routine_editor_switch_off)
    val parts = buildList {
        when {
            a.mode != null -> add(a.mode.label())
            a.on == true -> add(stringResource(Res.string.routine_editor_switch_on))
        }
        a.setpoint?.let { add(formatTemp(it)) }
    }
    return parts.joinToString(" · ").ifEmpty { "—" }
}

@Composable
private fun ScheduleCard(schedule: AirzoneSchedule, inst: Installation, canEdit: Boolean, onClick: () -> Unit, onEnabled: (Boolean) -> Unit) {
    SectionCard(Modifier.clickable(enabled = canEdit, onClick = onClick)) {
        ItemCardHeader(
            icon = FzIcons.CalendarClock,
            tint = Fz.colors.cool,
            title = schedule.name,
            subtitle = scheduleText(schedule.days, schedule.time) + (schedule.end?.let { " → ${it.time}" } ?: ""),
        ) {
            Switch(checked = schedule.enabled, onCheckedChange = onEnabled, enabled = inst.controllable)
        }
        val zones = schedule.zones.map { z ->
            inst.zone(z.systemId, z.zoneId)?.name ?: stringResource(Res.string.schedules_zone, z.zoneId.toInt())
        }
        Text((listOf(actionText(schedule.action)) + zones).joinToString(" · "), fontSize = 13.sp)
        schedule.note?.let { Caption(it) }
        if (!schedule.editable) Caption(stringResource(Res.string.schedules_readonly))
    }
}

private enum class Kind { On, Off, Mode }

/** Same model as the Airzone app: zones, days, time, and ON / OFF / mode with an optional setpoint. */
@OptIn(ExperimentalMaterial3Api::class, ExperimentalLayoutApi::class)
@Composable
private fun ScheduleEditor(
    inst: Installation,
    schedule: AirzoneSchedule?,
    onDismiss: () -> Unit,
    onSave: suspend (AirzoneScheduleInput) -> Unit,
    onDelete: (AirzoneSchedule) -> Unit,
) {
    val hub = koinInject<Hub>()
    val scope = rememberCoroutineScope()
    val action = schedule?.action
    var name by remember { mutableStateOf(schedule?.name ?: "") }
    var days by remember { mutableStateOf((schedule?.days ?: listOf(1, 2, 3, 4, 5)).toSet()) }
    var time by remember { mutableStateOf(schedule?.time ?: "07:00") }
    var zones by remember { mutableStateOf(schedule?.zones?.toSet() ?: emptySet()) }
    var kind by remember { mutableStateOf(if (action?.on == false) Kind.Off else if (action?.mode != null) Kind.Mode else Kind.On) }
    var mode by remember { mutableStateOf(action?.mode ?: Mode.Heating) }
    var setpoint by remember { mutableStateOf(action?.setpoint ?: 20.0) }
    var pickTime by remember { mutableStateOf(false) }
    var busy by remember { mutableStateOf(false) }
    val modes = inst.zones.flatMap { it.modes }.distinct().filter { it != Mode.Stop }.ifEmpty { listOf(Mode.Heating, Mode.Cooling) }
    val limits = SetpointLimits.of(inst.zones.filter { ScheduleZone(it.systemId, it.zoneId) in zones }.ifEmpty { inst.zones })

    fun save() {
        val target = limits.clamp(setpoint)
        val command = when (kind) {
            Kind.Off -> ZoneCommand(on = false)
            Kind.On -> ZoneCommand(on = true, setpoint = target)
            Kind.Mode -> ZoneCommand(mode = mode, setpoint = target)
        }
        val input = AirzoneScheduleInput(
            name = name.trim(),
            enabled = schedule?.enabled ?: true,
            days = days.sorted(),
            time = time,
            zones = zones.toList(),
            action = command,
        )
        busy = true
        scope.launch {
            hub.attempt { onSave(input) }
            busy = false
        }
    }

    EditorSheet(
        title = stringResource(if (schedule == null) Res.string.schedules_new else Res.string.schedule_editor_edit),
        canSave = !busy && name.isNotBlank() && days.isNotEmpty() && zones.isNotEmpty(),
        onSave = ::save,
        onDismiss = onDismiss,
        deleteName = schedule?.name,
        onDelete = { schedule?.let(onDelete) },
    ) {
        NameField(name, { name = it.take(NAME_MAX) }, stringResource(Res.string.schedule_editor_name_placeholder))
        Text(stringResource(Res.string.schedule_editor_days), style = MaterialTheme.typography.titleSmall)
        val short = stringArrayResource(Res.array.days_short)
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
            (1..7).forEach { d ->
                val on = d in days
                FilterChip(selected = on, onClick = { days = if (on) days - d else days + d }, label = { Text(short[d - 1]) })
            }
        }
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(stringResource(Res.string.schedule_editor_time), Modifier.weight(1f))
            OutlinedButton(onClick = { pickTime = true }, shape = Fz.shapes.button) { Text(time, fontSize = 18.sp) }
        }
        Text(stringResource(Res.string.schedule_editor_action), style = MaterialTheme.typography.titleSmall)
        SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
            val kinds = listOf(
                Kind.On to Res.string.routine_editor_switch_on,
                Kind.Off to Res.string.routine_editor_switch_off,
                Kind.Mode to Res.string.schedule_editor_mode,
            )
            kinds.forEachIndexed { i, (k, label) ->
                SegmentedButton(kind == k, { kind = k }, SegmentedButtonDefaults.itemShape(i, kinds.size)) { Text(stringResource(label)) }
            }
        }
        if (kind == Kind.Mode) {
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                modes.forEach { m -> FilterChip(selected = m == mode, onClick = { mode = m }, label = { Text(m.label()) }) }
            }
        }
        if (kind != Kind.Off) SetpointStepper(setpoint, { setpoint = it }, limits)
        ZoneChecklist(
            zones = inst.zones,
            selected = zones,
            keyOf = { z: Zone -> ScheduleZone(z.systemId, z.zoneId) },
            onChange = { zones = it },
        )
    }

    if (pickTime) {
        val (h, m) = time.split(":").map { it.toIntOrNull() ?: 0 }.let { it.getOrElse(0) { 7 } to it.getOrElse(1) { 0 } }
        val picker = rememberTimePickerState(initialHour = h, initialMinute = m, is24Hour = true)
        AlertDialog(
            onDismissRequest = { pickTime = false },
            text = { TimePicker(picker) },
            confirmButton = {
                TextButton(onClick = {
                    time = "${picker.hour.toString().padStart(2, '0')}:${picker.minute.toString().padStart(2, '0')}"
                    pickTime = false
                }) { Text(stringResource(Res.string.common_ok)) }
            },
            dismissButton = { TextButton(onClick = { pickTime = false }) { Text(stringResource(Res.string.common_cancel)) } },
        )
    }
}
