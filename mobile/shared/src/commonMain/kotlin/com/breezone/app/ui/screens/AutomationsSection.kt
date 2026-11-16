package com.breezone.app.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Switch
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
import androidx.compose.ui.draw.clip
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.breezone.app.data.Automation
import com.breezone.app.data.AutomationRun
import com.breezone.app.data.Hub
import com.breezone.app.data.Installation
import com.breezone.app.data.Presence
import com.breezone.app.resources.*
import com.breezone.app.ui.icons.FzIcons
import com.breezone.app.ui.relativeTime
import com.breezone.app.ui.theme.Fz
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull
import org.jetbrains.compose.resources.getString
import org.jetbrains.compose.resources.pluralStringResource
import org.jetbrains.compose.resources.stringResource
import org.koin.compose.koinInject

/** What the automations see changes all the time (a measure, a condition): looked at again. */
private const val REFRESH_MS = 30_000L

/** The devices a rule reads or drives, from its JSON. */
private fun devicesOf(a: Automation): List<Long> {
    fun of(o: JsonObject): List<Long> = listOfNotNull(
        o["device_id"]?.jsonPrimitive?.longOrNull,
        (o["measure"] as? JsonObject)?.get("device_id")?.jsonPrimitive?.longOrNull,
    ) + (o["conditions"] as? kotlinx.serialization.json.JsonArray).orEmpty().flatMap { (it as? JsonObject)?.let(::of).orEmpty() } +
        ((o["condition"] as? JsonObject)?.let(::of).orEmpty())
    return (a.triggers + a.conditions + a.actions + a.otherwise + a.over).flatMap(::of)
}

/**
 * The automations of an installation (those that read or drive one of its zones, and those of
 * the modules alone), with "I'm home". They are created and changed on the web.
 */
@Composable
fun AutomationsSection(serverId: String, inst: Installation) {
    val hub = koinInject<Hub>()
    val scope = rememberCoroutineScope()
    var list by remember { mutableStateOf<List<Automation>?>(null) }
    var presence by remember { mutableStateOf<Presence?>(null) }
    var paused by remember { mutableStateOf(false) }
    var unreadable by remember { mutableIntStateOf(0) }
    val admin = hub.server(serverId)?.user?.isAdmin == true
    var refreshKey by remember { mutableIntStateOf(0) }

    LaunchedEffect(serverId, inst.id, refreshKey) {
        val api = hub.api(serverId) ?: return@LaunchedEffect
        while (true) {
            runCatching {
                val zones = api.devices().associate { it.id to it.installationId }
                list = api.automations().filter { a ->
                    val installations = devicesOf(a).mapNotNull { zones[it] }
                    installations.isEmpty() || inst.id in installations
                }
                presence = api.presence()
                paused = api.automationSettings().pausedUntil != null
            }
            // Administrators only, and absent before server 0.11.
            if (admin) runCatching { unreadable = api.unreadableAutomations().size }
            delay(REFRESH_MS)
        }
    }

    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        SectionTitle(stringResource(Res.string.automations_title), FzIcons.Workflow)
        SectionCard {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(stringResource(Res.string.automations_home), Modifier.weight(1f))
                Switch(
                    checked = presence?.me == true,
                    onCheckedChange = { home -> hub.launchAttempt(scope) { hub.api(serverId)?.setPresence(home)?.let { presence = it } } },
                )
            }
            if (paused) Caption(stringResource(Res.string.automations_paused), color = Fz.colors.warning)
            if (unreadable > 0) Caption(pluralStringResource(Res.plurals.automations_unreadable, unreadable, unreadable), color = Fz.colors.warning)
        }
        val rules = list
        when {
            rules == null -> Unit
            rules.isEmpty() -> Muted(stringResource(Res.string.automations_empty))
            else -> rules.forEach { a ->
                AutomationCard(
                    automation = a,
                    onEnabled = { enabled ->
                        hub.launchAttempt(scope) {
                            hub.api(serverId)?.enableAutomation(a.id, enabled)
                            refreshKey++
                        }
                    },
                    onRun = {
                        scope.launch {
                            hub.attempt { hub.api(serverId)?.runAutomation(a.id) }?.let { r ->
                                val failed = r.results.count { !it.ok }
                                hub.notify(
                                    when {
                                        r.running -> getString(Res.string.automations_started, a.name)
                                        failed > 0 -> getString(Res.string.automations_failed, a.name, failed)
                                        else -> getString(Res.string.routines_ran, a.name)
                                    },
                                )
                            }
                            refreshKey++
                        }
                    },
                    loadRuns = { hub.attempt { hub.api(serverId)?.automationRuns(a.id) }.orEmpty() },
                )
            }
        }
        Caption(stringResource(Res.string.automations_on_web))
    }
}

@Composable
private fun Line(label: String, color: androidx.compose.ui.graphics.Color, lines: List<String>, muted: List<Boolean> = emptyList()) {
    if (lines.isEmpty()) return
    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(
            label,
            fontSize = 11.sp,
            color = color,
            modifier = Modifier.clip(RoundedCornerShape(8.dp)).background(color.copy(alpha = 0.12f)).padding(horizontal = 6.dp, vertical = 1.dp),
        )
        Column(Modifier.weight(1f)) {
            lines.forEachIndexed { i, line ->
                Text(line, fontSize = 14.sp, color = if (muted.getOrNull(i) == true) Fz.colors.muted else MaterialTheme.colorScheme.onSurface)
            }
        }
    }
}

@Composable
private fun AutomationCard(automation: Automation, onEnabled: (Boolean) -> Unit, onRun: () -> Unit, loadRuns: suspend () -> List<AutomationRun>) {
    val a = automation
    var history by remember { mutableStateOf<List<AutomationRun>?>(null) }
    var showHistory by remember { mutableStateOf(false) }
    LaunchedEffect(showHistory) { if (showHistory) history = loadRuns() }
    SectionCard {
        ItemCardHeader(FzIcons.Workflow, Fz.colors.fan, a.name) { Switch(checked = a.enabled, onCheckedChange = onEnabled) }
        if (a.status.running) Caption(stringResource(Res.string.automations_running), color = MaterialTheme.colorScheme.primary)
        val word = a.summary.joined
        Line(stringResource(Res.string.automations_when), Fz.colors.warning, a.summary.triggers.mapIndexed { i, t -> if (i == 0) t else "${stringResource(Res.string.automations_or)} $t" })
        Line(
            stringResource(Res.string.automations_if),
            Fz.colors.fan,
            a.summary.conditions.mapIndexed { i, c -> if (i == 0) c else "$word $c" },
            a.status.conditions.map { it != true },
        )
        Line(stringResource(Res.string.automations_then), MaterialTheme.colorScheme.primary, a.summary.actions)
        if (a.summary.over.isNotEmpty()) {
            Line(
                stringResource(Res.string.automations_over),
                MaterialTheme.colorScheme.primary,
                listOf(stringResource(Res.string.automations_over_when)) + a.summary.over,
            )
        }
        Line(stringResource(Res.string.automations_else), Fz.colors.muted, a.summary.otherwise)
        if (a.conflicts.isNotEmpty()) {
            Caption(stringResource(Res.string.automations_conflict, a.conflicts.joinToString(", ") { it.name }, a.conflicts.first().what), color = Fz.colors.warning)
        }
        Row(verticalAlignment = Alignment.CenterVertically) {
            Caption(
                a.lastRunAt?.let { stringResource(Res.string.automations_last, relativeTime(it), a.lastReason.orEmpty()) } ?: stringResource(Res.string.automations_never),
                Modifier.weight(1f),
            )
            IconTextButton(FzIcons.History, stringResource(Res.string.automations_history), { showHistory = !showHistory })
            IconTextButton(FzIcons.Play, stringResource(Res.string.automations_try), onRun)
        }
        if (showHistory) {
            val runs = history
            when {
                runs == null -> Unit
                runs.isEmpty() -> Caption(stringResource(Res.string.automations_history_empty))
                else -> runs.take(5).forEach { r -> RunLine(r) }
            }
        }
    }
}

@Composable
private fun RunLine(r: AutomationRun) {
    val branch = when (r.branch) {
        "then" -> stringResource(Res.string.automations_ran)
        "else" -> stringResource(Res.string.automations_else)
        "busy" -> stringResource(Res.string.automations_busy)
        else -> stringResource(Res.string.automations_skipped)
    }
    Column(Modifier.fillMaxWidth()) {
        Caption(listOf(branch, relativeTime(r.startedAt)).joinToString(" · "))
        Text(r.reason, fontSize = 13.sp)
        r.steps.forEach { s -> Caption((if (s.ok) "✓ " else "✗ ") + s.what + (s.error?.let { " — $it" } ?: "")) }
    }
}
