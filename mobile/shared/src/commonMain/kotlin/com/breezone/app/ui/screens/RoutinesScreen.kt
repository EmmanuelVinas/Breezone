package com.breezone.app.ui.screens

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Checkbox
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TimePicker
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.material3.rememberTimePickerState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.breezone.app.data.Action
import com.breezone.app.data.Hub
import com.breezone.app.data.Routine
import com.breezone.app.data.RoutineInput
import com.breezone.app.data.ServerState
import com.breezone.app.resources.*
import com.breezone.app.ui.formatTemp
import com.breezone.app.ui.icons.FzIcons
import com.breezone.app.ui.relativeTime
import com.breezone.app.ui.scheduleText
import com.breezone.app.ui.theme.Fz
import kotlinx.coroutines.launch
import org.jetbrains.compose.resources.getPluralString
import org.jetbrains.compose.resources.getString
import org.jetbrains.compose.resources.pluralStringResource
import org.jetbrains.compose.resources.stringArrayResource
import org.jetbrains.compose.resources.stringResource
import org.koin.compose.koinInject

private data class Editing(val serverId: String, val routine: Routine?)

private const val TYPE_HEADER = "header"
private const val TYPE_SERVER = "server"
private const val TYPE_EMPTY = "empty"
private const val TYPE_ROUTINE = "routine"

private fun zoneKey(installationId: Long, systemId: Long, zoneId: Long) = "$installationId-$systemId-$zoneId"

@Composable
fun RoutinesScreen() {
    val hub = koinInject<Hub>()
    val servers by hub.servers.collectAsState()
    val active = servers.filter { !it.authRequired }
    val routines = remember { mutableStateMapOf<String, List<Routine>>() }
    val scope = rememberCoroutineScope()
    var editing by remember { mutableStateOf<Editing?>(null) }
    var refreshKey by remember { mutableIntStateOf(0) }

    LaunchedEffect(active.map { it.account.id }, refreshKey) {
        active.forEach { s ->
            runCatching { hub.api(s.account.id)?.routines() }.getOrNull()?.let { routines[s.account.id] = it }
        }
    }
    fun reload() {
        refreshKey++
    }

    Box(Modifier.fillMaxSize()) {
        LazyColumn(
            Modifier.fillMaxSize().statusBarsPadding(),
            contentPadding = PaddingValues(20.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            item(key = TYPE_HEADER, contentType = TYPE_HEADER) {
                Column {
                    Text(stringResource(Res.string.routines_title), style = MaterialTheme.typography.headlineMedium)
                    Muted(stringResource(Res.string.routines_subtitle))
                }
            }
            active.forEach { s ->
                val serverId = s.account.id
                if (active.size > 1) {
                    item(key = "server-$serverId", contentType = TYPE_SERVER) {
                        Text(s.account.label, style = MaterialTheme.typography.titleSmall, color = Fz.colors.muted)
                    }
                }
                val list = routines[serverId]
                if (list != null && list.isEmpty()) {
                    item(key = "empty-$serverId", contentType = TYPE_EMPTY) { Muted(stringResource(Res.string.routines_empty)) }
                }
                items(list.orEmpty(), key = { "$serverId-${it.id}" }, contentType = { TYPE_ROUTINE }) { r ->
                    RoutineCard(
                        routine = r,
                        onRun = {
                            scope.launch {
                                runCatching { hub.api(serverId)?.runRoutine(r.id) }
                                    .onSuccess { res ->
                                        val failed = res?.results?.count { !it.ok } ?: 0
                                        hub.notify(
                                            if (failed == 0) getString(Res.string.routines_ran, r.name)
                                            else getPluralString(Res.plurals.routines_ran_with_failures, failed, r.name, failed),
                                        )
                                    }
                                    .onFailure { hub.notify(getString(Res.string.common_failure, it.message.orEmpty())) }
                                reload()
                            }
                        },
                        onEdit = { editing = Editing(serverId, r) },
                    )
                }
            }
            item(key = "bottom-space", contentType = "space") { Spacer(Modifier.height(80.dp)) }
        }
        if (active.isNotEmpty()) {
            FloatingActionButton(
                onClick = { editing = Editing(active.first().account.id, null) },
                modifier = Modifier.align(Alignment.BottomEnd).padding(20.dp),
                containerColor = MaterialTheme.colorScheme.primary,
            ) { Icon(FzIcons.Plus, stringResource(Res.string.routines_new)) }
        }
    }

    editing?.let { e ->
        RoutineEditor(
            servers = active,
            initialServerId = e.serverId,
            routine = e.routine,
            onDismiss = { editing = null },
            onSaved = {
                editing = null
                reload()
            },
        )
    }
}

@Composable
private fun RoutineCard(routine: Routine, onRun: () -> Unit, onEdit: () -> Unit) {
    SectionCard(Modifier.clickable(onClick = onEdit)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Icon(FzIcons.Sparkles, null, Modifier.size(18.dp), tint = Fz.colors.fan)
            Spacer(Modifier.size(8.dp))
            Column(Modifier.weight(1f)) {
                Text(routine.name, style = MaterialTheme.typography.titleSmall)
            }
        }
        Row(verticalAlignment = Alignment.CenterVertically) {
            val parts = buildList {
                add(pluralStringResource(Res.plurals.routines_zone_count, routine.actions.size, routine.actions.size))
                routine.lastRunAt?.let { add(stringResource(Res.string.routines_last, relativeTime(it))) }
            }
            Text(parts.joinToString(" · "), fontSize = 12.sp, color = Fz.colors.muted, modifier = Modifier.weight(1f))
            TextButton(onClick = onRun) {
                Icon(FzIcons.Play, null, Modifier.size(16.dp))
                Spacer(Modifier.size(4.dp))
                Text(stringResource(Res.string.routines_run))
            }
        }
    }
}

/** The mobile editor covers the common case: same action ("switch on at X°" or "switch off") on a set of zones. */
private fun isSimple(r: Routine?): Boolean {
    if (r == null || r.actions.isEmpty()) return true
    val first = r.actions.first()
    return r.actions.all { it.on == first.on && it.setpoint == first.setpoint && it.mode == null && (it.delayMinutes ?: 0) == 0L }
}

@OptIn(ExperimentalMaterial3Api::class, ExperimentalLayoutApi::class)
@Composable
private fun RoutineEditor(servers: List<ServerState>, initialServerId: String, routine: Routine?, onDismiss: () -> Unit, onSaved: () -> Unit) {
    val hub = koinInject<Hub>()
    val scope = rememberCoroutineScope()
    val sheet = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    var serverId by remember { mutableStateOf(initialServerId) }
    val server = servers.firstOrNull { it.account.id == serverId }
    val simple = isSimple(routine)
    var name by remember { mutableStateOf(routine?.name ?: "") }
    var switchOn by remember { mutableStateOf(routine?.actions?.firstOrNull()?.on ?: true) }
    var setpoint by remember { mutableStateOf(routine?.actions?.firstOrNull()?.setpoint ?: 20.0) }
    var selected by remember { mutableStateOf(routine?.actions?.map { zoneKey(it.installationId, it.systemId, it.zoneId) }?.toSet() ?: emptySet()) }
    var confirmDelete by remember { mutableStateOf(false) }
    var busy by remember { mutableStateOf(false) }

    fun save() {
        val zones = server?.installations?.flatMap { it.zones }.orEmpty().filter { zoneKey(it.installationId, it.systemId, it.zoneId) in selected }
        val actions = if (simple) {
            zones.map { Action(it.installationId, it.systemId, it.zoneId, on = switchOn, setpoint = if (switchOn) setpoint else null) }
        } else {
            routine!!.actions
        }
        val input = RoutineInput(name.trim(), routine?.enabled ?: true, actions)
        busy = true
        scope.launch {
            runCatching {
                val api = hub.api(serverId) ?: error(getString(Res.string.error_server_unavailable))
                if (routine == null) api.createRoutine(input) else api.updateRoutine(routine.id, input)
            }.onSuccess {
                hub.notify(getString(Res.string.routines_saved))
                onSaved()
            }.onFailure { hub.notify(getString(Res.string.common_failure, it.message.orEmpty())) }
            busy = false
        }
    }

    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = sheet) {
        Column(
            Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).padding(horizontal = 20.dp).navigationBarsPadding(),
            verticalArrangement = Arrangement.spacedBy(14.dp),
        ) {
            Text(
                stringResource(if (routine == null) Res.string.routines_new else Res.string.routine_editor_edit),
                style = MaterialTheme.typography.titleLarge,
            )
            if (servers.size > 1 && routine == null) {
                FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    servers.forEach { s ->
                        FilterChip(
                            selected = s.account.id == serverId,
                            onClick = {
                                serverId = s.account.id
                                selected = emptySet()
                            },
                            label = { Text(s.account.label) },
                        )
                    }
                }
            }
            OutlinedTextField(
                name,
                { name = it },
                label = { Text(stringResource(Res.string.routine_editor_name)) },
                placeholder = { Text(stringResource(Res.string.routine_editor_name_placeholder)) },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
                shape = RoundedCornerShape(16.dp),
            )
            if (simple) {
                SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
                    SegmentedButton(switchOn, { switchOn = true }, SegmentedButtonDefaults.itemShape(0, 2)) {
                        Text(stringResource(Res.string.routine_editor_switch_on))
                    }
                    SegmentedButton(!switchOn, { switchOn = false }, SegmentedButtonDefaults.itemShape(1, 2)) {
                        Text(stringResource(Res.string.routine_editor_switch_off))
                    }
                }
                if (switchOn) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text(stringResource(Res.string.routine_editor_setpoint), Modifier.weight(1f))
                        IconButton(onClick = { setpoint = (setpoint - 0.5).coerceAtLeast(15.0) }) {
                            Icon(FzIcons.Minus, stringResource(Res.string.routine_editor_less))
                        }
                        Text(formatTemp(setpoint), fontSize = 18.sp)
                        IconButton(onClick = { setpoint = (setpoint + 0.5).coerceAtMost(30.0) }) {
                            Icon(FzIcons.Plus, stringResource(Res.string.routine_editor_more))
                        }
                    }
                }
                Text(stringResource(Res.string.routine_editor_zones), style = MaterialTheme.typography.titleSmall)
                server?.installations?.forEach { inst ->
                    if (server.installations.size > 1) Text(inst.name, fontSize = 12.sp, color = Fz.colors.muted)
                    inst.zones.forEach { z ->
                        val key = zoneKey(z.installationId, z.systemId, z.zoneId)
                        val checked = key in selected
                        Row(
                            Modifier.fillMaxWidth().clickable { selected = if (checked) selected - key else selected + key },
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Checkbox(checked = checked, onCheckedChange = { selected = if (it) selected + key else selected - key })
                            Text(listOfNotNull(z.group, z.name).joinToString(" · "))
                        }
                    }
                }
            } else {
                Muted(stringResource(Res.string.routine_editor_complex))
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                if (routine != null) {
                    TextButton(onClick = { confirmDelete = true }) {
                        Icon(FzIcons.Trash2, null, Modifier.size(16.dp), tint = MaterialTheme.colorScheme.error)
                        Spacer(Modifier.size(4.dp))
                        Text(stringResource(Res.string.common_delete), color = MaterialTheme.colorScheme.error)
                    }
                }
                Spacer(Modifier.weight(1f))
                Button(
                    onClick = ::save,
                    enabled = !busy && name.isNotBlank() && (!simple || selected.isNotEmpty()),
                    shape = RoundedCornerShape(14.dp),
                ) { Text(stringResource(Res.string.common_save)) }
            }
            Spacer(Modifier.height(16.dp))
        }
    }

    if (confirmDelete && routine != null) {
        AlertDialog(
            onDismissRequest = { confirmDelete = false },
            title = { Text(stringResource(Res.string.routine_editor_delete_confirm, routine.name)) },
            confirmButton = {
                TextButton(onClick = {
                    scope.launch {
                        runCatching { hub.api(serverId)?.deleteRoutine(routine.id) }
                            .onFailure { hub.notify(getString(Res.string.common_failure, it.message.orEmpty())) }
                        onSaved()
                    }
                }) { Text(stringResource(Res.string.common_delete)) }
            },
            dismissButton = { TextButton(onClick = { confirmDelete = false }) { Text(stringResource(Res.string.common_cancel)) } },
        )
    }
}
