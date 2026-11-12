package com.breezone.app.ui.screens

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.breezone.app.APP_VERSION
import com.breezone.app.data.CloudStatus
import com.breezone.app.data.Hub
import com.breezone.app.data.UpdateNeeded
import com.breezone.app.data.Live
import com.breezone.app.data.ServerState
import com.breezone.app.data.SyncReport
import com.breezone.app.resources.*
import com.breezone.app.ui.components.StatusDot
import com.breezone.app.ui.icons.FzIcons
import com.breezone.app.ui.relativeTime
import com.breezone.app.ui.theme.Fz
import kotlinx.coroutines.launch
import org.jetbrains.compose.resources.getString
import org.jetbrains.compose.resources.stringResource
import org.koin.compose.koinInject


@Composable
fun SettingsScreen(onAddServer: () -> Unit, onReauth: (String) -> Unit) {
    val hub = koinInject<Hub>()
    val servers by hub.servers.collectAsState()
    var removing by remember { mutableStateOf<ServerState?>(null) }

    Column(
        Modifier.fillMaxSize().statusBarsPadding().verticalScroll(rememberScrollState()).padding(20.dp),
        verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        Text(stringResource(Res.string.settings_title), style = MaterialTheme.typography.headlineMedium)

        SectionCard {
            SectionTitle(stringResource(Res.string.settings_servers_title), FzIcons.Server)
            Muted(stringResource(Res.string.settings_servers_hint))
            servers.forEachIndexed { i, s ->
                if (i > 0) HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
                Row(verticalAlignment = Alignment.CenterVertically) {
                    StatusDot(online = s.live == Live.Live && !s.authRequired, unknown = s.live == Live.Connecting)
                    Spacer(Modifier.size(10.dp))
                    Column(Modifier.weight(1f)) {
                        Text(s.account.label, style = MaterialTheme.typography.titleSmall)
                        Caption(
                            stringResource(
                                if (s.user?.isAdmin == true) Res.string.settings_server_user_admin else Res.string.settings_server_user,
                                s.account.username,
                                s.account.url,
                            ),
                            maxLines = 1,
                        )
                        if (s.authRequired) Caption(stringResource(Res.string.settings_session_expired), color = Fz.colors.warning)
                        s.serverVersion?.let { Caption(stringResource(Res.string.settings_server_version, it)) }
                        if (s.user?.isAdmin == true && !s.authRequired) ServerUpdateNotice(hub, s.account.id)
                        when {
                            s.update == UpdateNeeded.App -> Caption(stringResource(Res.string.update_app_text), color = Fz.colors.warning)
                            s.update == UpdateNeeded.Server -> Caption(stringResource(Res.string.update_server_text, s.serverVersion ?: "?"), color = Fz.colors.warning)
                            s.appUpdateAvailable -> Caption(stringResource(Res.string.update_app_available))
                        }
                    }
                    if (s.authRequired) TextButton(onClick = { onReauth(s.account.id) }) { Text(stringResource(Res.string.settings_reconnect)) }
                    TextButton(onClick = { removing = s }) { Text(stringResource(Res.string.settings_remove), color = MaterialTheme.colorScheme.error) }
                }
            }
            OutlinedButton(onClick = onAddServer, shape = Fz.shapes.button) {
                Icon(FzIcons.Plus, null, Modifier.size(16.dp))
                Spacer(Modifier.size(6.dp))
                Text(stringResource(Res.string.settings_add_server))
            }
        }

        servers.filter { !it.authRequired }.forEach { s -> AirzoneAccount(hub, s, showServer = servers.size > 1) }

        SectionCard {
            SectionTitle(stringResource(Res.string.settings_security_title), FzIcons.CircleCheck)
            Muted(stringResource(Res.string.settings_security_text))
        }
        Muted(stringResource(Res.string.settings_version, APP_VERSION), Modifier.padding(start = 4.dp))
        Spacer(Modifier.height(24.dp))
    }

    removing?.let { s ->
        ConfirmDialog(
            title = stringResource(Res.string.settings_remove_title, s.account.label),
            text = stringResource(Res.string.settings_remove_text),
            confirm = stringResource(Res.string.settings_remove),
            onConfirm = {
                hub.removeServer(s.account.id)
                removing = null
            },
            onDismiss = { removing = null },
        )
    }
}

/** A newer Breezone for this server: installed from its web UI (administrators). */
@Composable
private fun ServerUpdateNotice(hub: Hub, serverId: String) {
    var latest by remember(serverId) { mutableStateOf<String?>(null) }
    LaunchedEffect(serverId) {
        // Servers before 0.9.0 have no such route: nothing to show.
        latest = runCatching { hub.api(serverId)?.serverUpdate() }.getOrNull()?.takeIf { it.available }?.latest
    }
    latest?.let { Caption(stringResource(Res.string.settings_server_update, it), color = MaterialTheme.colorScheme.primary) }
}

/**
 * Airzone account of a server: read-only import of the installations and their settings
 * (names, city, groups of zones). Breezone never drives the zones through the cloud.
 */
@Composable
private fun AirzoneAccount(hub: Hub, server: ServerState, showServer: Boolean) {
    val api = hub.api(server.account.id) ?: return
    val scope = rememberCoroutineScope()
    var status by remember { mutableStateOf<CloudStatus?>(null) }
    var report by remember { mutableStateOf<SyncReport?>(null) }
    var connecting by remember { mutableStateOf(false) }
    var busy by remember { mutableStateOf(false) }
    val admin = server.user?.isAdmin == true

    LaunchedEffect(server.account.id) { status = runCatching { api.cloud() }.getOrNull() }

    SectionCard {
        SectionTitle(
            if (showServer) stringResource(Res.string.cloud_title_server, server.account.label) else stringResource(Res.string.cloud_title),
            FzIcons.Cloud,
        )
        Muted(stringResource(Res.string.cloud_hint))
        val s = status
        if (s?.email != null) {
            Text(stringResource(Res.string.cloud_account, s.email), fontSize = 14.sp)
            Text(
                stringResource(if (s.autoSync) Res.string.cloud_last_sync_daily else Res.string.cloud_last_sync, relativeTime(s.lastSync)),
                fontSize = 12.sp,
                color = Fz.colors.muted,
            )
        }
        s?.lastError?.let { Text(it, fontSize = 12.sp, color = MaterialTheme.colorScheme.error) }
        report?.let { r -> ReportText(r) }
        s?.installations?.forEach { c ->
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(if (c.reachable) FzIcons.House else FzIcons.CloudOff, null, Modifier.size(16.dp), tint = Fz.colors.muted)
                Spacer(Modifier.size(8.dp))
                Text(c.name, Modifier.weight(1f), fontSize = 14.sp)
                Caption(stringResource(if (c.reachable) Res.string.cloud_driven else Res.string.cloud_not_reached))
            }
        }
        if (admin) {
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Button(onClick = { connecting = true }, shape = Fz.shapes.button) {
                    Text(stringResource(if (s?.email == null) Res.string.cloud_connect else Res.string.cloud_reconnect))
                }
                if (s?.autoSync == true) {
                    OutlinedButton(
                        onClick = {
                            busy = true
                            scope.launch {
                                runCatching { api.cloudSync() }
                                    .onSuccess {
                                        report = it.report
                                        status = it.status
                                    }
                                    .onFailure { hub.notify(getString(Res.string.cloud_sync_failed, it.message.orEmpty())) }
                                busy = false
                            }
                        },
                        enabled = !busy,
                        shape = Fz.shapes.button,
                    ) {
                        if (busy) CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp) else Text(stringResource(Res.string.cloud_sync))
                    }
                }
            }
        } else {
            Muted(stringResource(Res.string.cloud_admin_only))
        }
    }

    if (connecting) {
        var email by remember { mutableStateOf(status?.email ?: "") }
        var password by remember { mutableStateOf("") }
        var remember by remember { mutableStateOf(true) }
        var error by remember { mutableStateOf<String?>(null) }
        AlertDialog(
            onDismissRequest = { if (!busy) connecting = false },
            title = { Text(stringResource(Res.string.cloud_title)) },
            text = {
                Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                    Text(stringResource(Res.string.cloud_dialog_text), fontSize = 13.sp)
                    OutlinedTextField(
                        email,
                        { email = it },
                        label = { Text(stringResource(Res.string.cloud_email)) },
                        singleLine = true,
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Email, imeAction = ImeAction.Next),
                        modifier = Modifier.fillMaxWidth(),
                        shape = Fz.shapes.field,
                    )
                    PasswordField(password, { password = it }, stringResource(Res.string.cloud_password))
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Checkbox(remember, { remember = it })
                        Text(stringResource(Res.string.cloud_remember), fontSize = 13.sp)
                    }
                    error?.let { Text(it, color = MaterialTheme.colorScheme.error, fontSize = 13.sp) }
                }
            },
            confirmButton = {
                TextButton(
                    enabled = !busy && email.isNotBlank() && password.isNotEmpty(),
                    onClick = {
                        busy = true
                        error = null
                        scope.launch {
                            runCatching { api.cloudConnect(email, password, remember) }
                                .onSuccess {
                                    report = it.report
                                    status = it.status
                                    connecting = false
                                }
                                .onFailure { error = it.message }
                            busy = false
                        }
                    },
                ) { Text(stringResource(if (busy) Res.string.cloud_importing else Res.string.cloud_import)) }
            },
            dismissButton = { TextButton(onClick = { connecting = false }, enabled = !busy) { Text(stringResource(Res.string.common_cancel)) } },
        )
    }
}

@Composable
private fun ReportText(r: SyncReport) {
    val lines = buildList {
        if (r.created.isNotEmpty()) add(stringResource(Res.string.cloud_report_created, r.created.joinToString()))
        if (r.updated.isNotEmpty()) add(stringResource(Res.string.cloud_report_updated, r.updated.joinToString()))
        if (r.unreachable.isNotEmpty()) add(stringResource(Res.string.cloud_report_unreachable, r.unreachable.joinToString()))
    }.ifEmpty { listOf(stringResource(Res.string.cloud_report_empty)) }
    Text(lines.joinToString("\n"), fontSize = 13.sp, color = Fz.colors.success)
    if (r.notes.isNotEmpty()) Muted(r.notes.joinToString("\n"))
}
