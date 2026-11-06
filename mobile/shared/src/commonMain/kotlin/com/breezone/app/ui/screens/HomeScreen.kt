package com.breezone.app.ui.screens

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.breezone.app.data.CloudInstallation
import com.breezone.app.data.Hub
import com.breezone.app.data.Live
import com.breezone.app.resources.*
import com.breezone.app.ui.components.InstallationCard
import com.breezone.app.ui.components.RemoteInstallationCard
import com.breezone.app.ui.components.StatusDot
import com.breezone.app.ui.icons.FzIcons
import com.breezone.app.ui.theme.Fz
import org.jetbrains.compose.resources.stringResource
import org.koin.compose.koinInject

private const val TYPE_HEADER = "header"
private const val TYPE_REAUTH = "reauth"
private const val TYPE_LOADING = "loading"
private const val TYPE_INSTALLATION = "installation"
private const val TYPE_REMOTE = "remote"
private const val TYPE_EMPTY = "empty"

@Composable
fun HomeScreen(onOpenInstallation: (serverId: String, installationId: Long) -> Unit, onReauth: (String) -> Unit, onAddServer: () -> Unit) {
    val hub = koinInject<Hub>()
    // In-memory StateFlow of the Hub: collectAsState is the right tool (no background producer).
    val servers by hub.servers.collectAsState()
    var remoteInfo by remember { mutableStateOf<CloudInstallation?>(null) }
    val multi = servers.size > 1
    val allNames = servers.flatMap { s -> s.installations.map { it.name.lowercase() } }.toSet()
    val remote = servers.flatMap { it.remote }.distinctBy { it.cloudId }.filter { it.name.lowercase() !in allNames }
    val loading = servers.isNotEmpty() && servers.none { it.loaded } && servers.none { it.authRequired }
    val live = servers.isNotEmpty() && servers.filterNot { it.authRequired }.all { it.live == Live.Live }
    val expired = servers.filter { it.authRequired }
    val empty = servers.isEmpty() || (!loading && servers.all { it.loaded && it.installations.isEmpty() } && remote.isEmpty())

    LazyColumn(
        Modifier.fillMaxSize().statusBarsPadding(),
        contentPadding = PaddingValues(20.dp),
        verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        item(key = TYPE_HEADER, contentType = TYPE_HEADER) {
            Row(Modifier.fillMaxWidth().padding(bottom = 6.dp), verticalAlignment = Alignment.CenterVertically) {
                Text(stringResource(Res.string.home_title), style = MaterialTheme.typography.headlineMedium, modifier = Modifier.weight(1f))
                if (servers.isNotEmpty()) {
                    StatusDot(online = live, unknown = !live && servers.any { it.live == Live.Connecting })
                    Spacer(Modifier.size(6.dp))
                    Text(
                        stringResource(if (live) Res.string.live_on else Res.string.live_connecting),
                        style = MaterialTheme.typography.labelSmall,
                        color = Fz.colors.muted,
                    )
                }
            }
        }
        items(expired, key = { "reauth-${it.account.id}" }, contentType = { TYPE_REAUTH }) { s ->
            SectionCard {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Icon(FzIcons.TriangleAlert, null, tint = Fz.colors.warning, modifier = Modifier.size(18.dp))
                    Spacer(Modifier.size(8.dp))
                    Text(stringResource(Res.string.home_reauth_title, s.account.label), style = MaterialTheme.typography.titleSmall)
                }
                Muted(stringResource(Res.string.home_reauth_text))
                Button(onClick = { onReauth(s.account.id) }, shape = RoundedCornerShape(14.dp)) {
                    Text(stringResource(Res.string.home_reauth_button))
                }
            }
        }
        if (loading) {
            item(key = TYPE_LOADING, contentType = TYPE_LOADING) {
                Row(Modifier.fillMaxWidth().padding(40.dp), horizontalArrangement = Arrangement.Center) { CircularProgressIndicator() }
            }
        }
        servers.forEach { s ->
            items(s.installations, key = { "${s.account.id}-${it.id}" }, contentType = { TYPE_INSTALLATION }) { inst ->
                InstallationCard(inst, if (multi) s.account.label else null) { onOpenInstallation(s.account.id, inst.id) }
            }
        }
        items(remote, key = { "remote-${it.cloudId}" }, contentType = { TYPE_REMOTE }) { r ->
            RemoteInstallationCard(r) { remoteInfo = r }
        }
        if (empty) {
            item(key = TYPE_EMPTY, contentType = TYPE_EMPTY) {
                SectionCard {
                    Icon(FzIcons.House, null, tint = Fz.colors.muted, modifier = Modifier.size(32.dp))
                    Text(stringResource(Res.string.home_empty_title), style = MaterialTheme.typography.titleMedium)
                    Muted(stringResource(if (servers.isEmpty()) Res.string.home_empty_no_server else Res.string.home_empty_text))
                    if (servers.isEmpty()) Button(onClick = onAddServer) { Text(stringResource(Res.string.home_add_server)) }
                }
            }
        }
        item(key = "bottom-space", contentType = "space") { Spacer(Modifier.height(12.dp)) }
    }

    remoteInfo?.let { r ->
        AlertDialog(
            onDismissRequest = { remoteInfo = null },
            title = { Text(r.name) },
            text = { Text(stringResource(Res.string.home_remote_text)) },
            confirmButton = {
                TextButton(onClick = { remoteInfo = null; onAddServer() }) { Text(stringResource(Res.string.home_add_server)) }
            },
            dismissButton = { OutlinedButton(onClick = { remoteInfo = null }) { Text(stringResource(Res.string.common_close)) } },
        )
    }
}
