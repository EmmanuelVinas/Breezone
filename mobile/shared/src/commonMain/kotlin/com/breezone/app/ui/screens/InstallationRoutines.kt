package com.breezone.app.ui.screens

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Text
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
import com.breezone.app.data.Action
import com.breezone.app.data.Hub
import com.breezone.app.data.Installation
import com.breezone.app.data.Routine
import com.breezone.app.data.RoutineInput
import com.breezone.app.data.Zone
import com.breezone.app.resources.*
import com.breezone.app.ui.SetpointLimits
import com.breezone.app.ui.icons.FzIcons
import com.breezone.app.ui.relativeTime
import com.breezone.app.ui.theme.Fz
import kotlinx.coroutines.launch
import org.jetbrains.compose.resources.getPluralString
import org.jetbrains.compose.resources.getString
import org.jetbrains.compose.resources.pluralStringResource
import org.jetbrains.compose.resources.stringResource
import org.koin.compose.koinInject

/** A zone of the edited installation (its actions on other installations are kept aside). */
private fun zoneKey(systemId: Long, zoneId: Long) = systemId to zoneId

@Composable
fun InstallationRoutines(serverId: String, inst: Installation, canEdit: Boolean, modifier: Modifier = Modifier) {
    val hub = koinInject<Hub>()
    val scope = rememberCoroutineScope()
    var routines by remember { mutableStateOf<List<Routine>?>(null) }
    var editing by remember { mutableStateOf<Routine?>(null) }
    var creating by remember { mutableStateOf(false) }
    var refreshKey by remember { mutableIntStateOf(0) }

    LaunchedEffect(serverId, refreshKey) {
        runCatching { hub.api(serverId)?.routines() }.getOrNull()?.let { list ->
            routines = list.filter { r -> r.actions.any { it.installationId == inst.id } }
        }
    }
    fun reload() {
        refreshKey++
    }

    SectionList(
        hint = stringResource(Res.string.routines_installation_hint),
        addLabel = stringResource(Res.string.routines_new),
        onAdd = if (canEdit) ({ creating = true }) else null,
        modifier = modifier,
    ) {
        val list = routines
        if (list != null && list.isEmpty()) {
            item(key = "empty", contentType = "empty") { Muted(stringResource(Res.string.routines_installation_empty)) }
        }
        items(list.orEmpty(), key = { it.id }, contentType = { "routine" }) { r ->
            RoutineCard(
                routine = r,
                onRun = {
                    scope.launch {
                        hub.attempt { hub.api(serverId)?.runRoutine(r.id) }?.let { res ->
                            val failed = res.results.count { !it.ok }
                            hub.notify(
                                if (failed == 0) getString(Res.string.routines_ran, r.name)
                                else getPluralString(Res.plurals.routines_ran_with_failures, failed, r.name, failed),
                            )
                        }
                        reload()
                    }
                },
                onEdit = if (canEdit) ({ editing = r }) else null,
            )
        }
    }

    if (creating || editing != null) {
        val close = {
            creating = false
            editing = null
        }
        RoutineEditor(
            serverId = serverId,
            inst = inst,
            routine = editing,
            onDismiss = close,
            onSaved = {
                close()
                reload()
            },
        )
    }
}

@Composable
private fun RoutineCard(routine: Routine, onRun: () -> Unit, onEdit: (() -> Unit)?) {
    SectionCard(Modifier.clickable(enabled = onEdit != null) { onEdit?.invoke() }) {
        ItemCardHeader(FzIcons.Sparkles, Fz.colors.fan, routine.name)
        Row(verticalAlignment = Alignment.CenterVertically) {
            val parts = buildList {
                add(pluralStringResource(Res.plurals.routines_zone_count, routine.actions.size, routine.actions.size))
                routine.lastRunAt?.let { add(stringResource(Res.string.routines_last, relativeTime(it))) }
            }
            Caption(parts.joinToString(" · "), Modifier.weight(1f))
            IconTextButton(FzIcons.Play, stringResource(Res.string.routines_run), onRun)
        }
    }
}

/** The mobile editor covers the common case: same action ("switch on at X°" or "switch off") on a set of zones. */
private fun isSimple(actions: List<Action>): Boolean {
    if (actions.isEmpty()) return true
    val first = actions.first()
    return actions.all { it.on == first.on && it.setpoint == first.setpoint && it.mode == null && (it.delayMinutes ?: 0) == 0L }
}

/** Edits the actions of a routine on one installation; its actions on other installations are kept. */
@Composable
private fun RoutineEditor(serverId: String, inst: Installation, routine: Routine?, onDismiss: () -> Unit, onSaved: () -> Unit) {
    val hub = koinInject<Hub>()
    val scope = rememberCoroutineScope()
    val (mine, others) = (routine?.actions ?: emptyList()).partition { it.installationId == inst.id }
    val simple = isSimple(mine)
    var name by remember { mutableStateOf(routine?.name ?: "") }
    var switchOn by remember { mutableStateOf(mine.firstOrNull()?.on ?: true) }
    var setpoint by remember { mutableStateOf(mine.firstOrNull()?.setpoint ?: 20.0) }
    var selected by remember { mutableStateOf(mine.map { zoneKey(it.systemId, it.zoneId) }.toSet()) }
    var busy by remember { mutableStateOf(false) }
    val chosen = inst.zones.filter { zoneKey(it.systemId, it.zoneId) in selected }
    val limits = SetpointLimits.of(chosen.ifEmpty { inst.zones })

    fun save() {
        val actions = if (simple) {
            val target = limits.clamp(setpoint)
            others + chosen.map { Action(it.installationId, it.systemId, it.zoneId, on = switchOn, setpoint = if (switchOn) target else null) }
        } else {
            routine!!.actions
        }
        val input = RoutineInput(name.trim(), routine?.enabled ?: true, actions)
        busy = true
        scope.launch {
            hub.attempt {
                val api = hub.api(serverId) ?: error(getString(Res.string.error_server_unavailable))
                if (routine == null) api.createRoutine(input) else api.updateRoutine(routine.id, input)
            }?.let {
                hub.notify(getString(Res.string.routines_saved))
                onSaved()
            }
            busy = false
        }
    }

    EditorSheet(
        title = stringResource(if (routine == null) Res.string.routines_new else Res.string.routine_editor_edit),
        canSave = !busy && name.isNotBlank() && (!simple || selected.isNotEmpty()),
        onSave = ::save,
        onDismiss = onDismiss,
        deleteName = routine?.name,
        onDelete = {
            val id = routine?.id ?: return@EditorSheet
            scope.launch {
                hub.attempt { hub.api(serverId)?.deleteRoutine(id) }
                onSaved()
            }
        },
    ) {
        NameField(name, { name = it }, stringResource(Res.string.routine_editor_name_placeholder))
        if (simple) {
            SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
                SegmentedButton(switchOn, { switchOn = true }, SegmentedButtonDefaults.itemShape(0, 2)) {
                    Text(stringResource(Res.string.routine_editor_switch_on))
                }
                SegmentedButton(!switchOn, { switchOn = false }, SegmentedButtonDefaults.itemShape(1, 2)) {
                    Text(stringResource(Res.string.routine_editor_switch_off))
                }
            }
            if (switchOn) SetpointStepper(setpoint, { setpoint = it }, limits)
            ZoneChecklist(
                zones = inst.zones,
                selected = selected,
                keyOf = { z: Zone -> zoneKey(z.systemId, z.zoneId) },
                onChange = { selected = it },
                note = if (others.isNotEmpty()) stringResource(Res.string.routine_editor_other_installations) else null,
            )
        } else {
            Muted(stringResource(Res.string.routine_editor_complex))
        }
    }
}
