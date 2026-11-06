package com.breezone.app.ui.screens

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.breezone.app.data.ApiException
import com.breezone.app.data.AuthRequiredException
import com.breezone.app.data.Hub
import com.breezone.app.data.localNetworkBlocked
import com.breezone.app.resources.*
import com.breezone.app.ui.icons.FzIcons
import com.breezone.app.ui.theme.Fz
import kotlinx.coroutines.launch
import org.jetbrains.compose.resources.getString
import org.jetbrains.compose.resources.stringResource
import org.koin.compose.koinInject

/**
 * Connects the app to a Breezone server (local address or Cloudflare Tunnel URL).
 * Also used to sign in again when a session expired, and to create the administrator of a
 * brand new server.
 */
@Composable
fun AddServerScreen(reauthServerId: String?, canGoBack: Boolean, onBack: () -> Unit, onDone: () -> Unit) {
    val hub = koinInject<Hub>()
    val existing = reauthServerId?.let { hub.server(it)?.account }
    val scope = rememberCoroutineScope()
    var url by remember { mutableStateOf(existing?.url ?: "") }
    var username by remember { mutableStateOf(existing?.username ?: "") }
    var password by remember { mutableStateOf("") }
    var confirm by remember { mutableStateOf("") }
    var showPassword by remember { mutableStateOf(false) }
    var needsSetup by remember { mutableStateOf(false) }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }

    fun submit() {
        error = null
        busy = true
        scope.launch {
            try {
                if (!needsSetup) {
                    val status = hub.discovery.probe(url)
                    if (status.needsSetup) {
                        needsSetup = true
                        busy = false
                        return@launch
                    }
                } else if (password != confirm) {
                    error = getString(Res.string.add_server_error_mismatch)
                    return@launch
                }
                hub.signIn(url, username, password, setup = needsSetup)
                onDone()
            } catch (e: Exception) {
                error = when {
                    e is ApiException || e is AuthRequiredException -> e.message
                    localNetworkBlocked() -> getString(Res.string.add_server_error_local_network)
                    else -> null
                } ?: e.message ?: getString(Res.string.add_server_error_unreachable)
            } finally {
                busy = false
            }
        }
    }

    Column(
        Modifier
            .fillMaxSize()
            .statusBarsPadding()
            .imePadding()
            .verticalScroll(rememberScrollState())
            .padding(24.dp),
        verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        if (canGoBack) {
            IconButton(onClick = onBack) { Icon(FzIcons.ChevronLeft, stringResource(Res.string.common_back)) }
        } else {
            Spacer(Modifier.height(32.dp))
        }
        Row(verticalAlignment = Alignment.CenterVertically) {
            Icon(FzIcons.Wind, null, Modifier.size(36.dp), tint = MaterialTheme.colorScheme.primary)
            Spacer(Modifier.size(10.dp))
            Text(stringResource(Res.string.app_name), fontSize = 30.sp, style = MaterialTheme.typography.headlineMedium)
        }
        Text(
            when {
                existing != null -> stringResource(Res.string.add_server_title_reauth, existing.label)
                needsSetup -> stringResource(Res.string.add_server_title_setup)
                else -> stringResource(Res.string.add_server_title)
            },
            style = MaterialTheme.typography.titleLarge,
        )
        Muted(
            stringResource(
                when {
                    existing != null -> Res.string.add_server_hint_reauth
                    needsSetup -> Res.string.add_server_hint_setup
                    else -> Res.string.add_server_hint
                },
            ),
        )
        OutlinedTextField(
            value = url,
            onValueChange = { url = it; needsSetup = false },
            label = { Text(stringResource(Res.string.add_server_url)) },
            placeholder = { Text(stringResource(Res.string.add_server_url_placeholder)) },
            singleLine = true,
            enabled = existing == null,
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Next),
            modifier = Modifier.fillMaxWidth(),
            shape = RoundedCornerShape(16.dp),
        )
        OutlinedTextField(
            value = username,
            onValueChange = { username = it },
            label = { Text(stringResource(Res.string.add_server_username)) },
            singleLine = true,
            enabled = existing == null,
            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Next),
            modifier = Modifier.fillMaxWidth(),
            shape = RoundedCornerShape(16.dp),
        )
        OutlinedTextField(
            value = password,
            onValueChange = { password = it },
            label = { Text(stringResource(if (needsSetup) Res.string.add_server_password_setup else Res.string.add_server_password)) },
            singleLine = true,
            visualTransformation = if (showPassword) VisualTransformation.None else PasswordVisualTransformation(),
            trailingIcon = {
                IconButton(onClick = { showPassword = !showPassword }) {
                    Icon(if (showPassword) FzIcons.EyeOff else FzIcons.Eye, stringResource(Res.string.add_server_show_password))
                }
            },
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password, imeAction = ImeAction.Done),
            modifier = Modifier.fillMaxWidth(),
            shape = RoundedCornerShape(16.dp),
        )
        if (needsSetup) {
            OutlinedTextField(
                value = confirm,
                onValueChange = { confirm = it },
                label = { Text(stringResource(Res.string.add_server_confirm)) },
                singleLine = true,
                visualTransformation = PasswordVisualTransformation(),
                modifier = Modifier.fillMaxWidth(),
                shape = RoundedCornerShape(16.dp),
            )
        }
        error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
        Button(
            onClick = ::submit,
            enabled = !busy && url.isNotBlank() && username.isNotBlank() && password.isNotEmpty(),
            modifier = Modifier.fillMaxWidth().height(54.dp),
            shape = RoundedCornerShape(16.dp),
        ) {
            if (busy) {
                CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp)
            } else {
                Text(stringResource(if (needsSetup) Res.string.add_server_create else Res.string.add_server_sign_in))
            }
        }
        Row(verticalAlignment = Alignment.Top) {
            Icon(FzIcons.CircleCheck, null, Modifier.size(16.dp).padding(top = 2.dp), tint = Fz.colors.success)
            Spacer(Modifier.size(8.dp))
            Muted(stringResource(Res.string.add_server_stay_signed_in))
        }
        if (existing != null) {
            TextButton(onClick = { hub.removeServer(existing.id); onDone() }) { Text(stringResource(Res.string.add_server_forget)) }
        }
    }
}
