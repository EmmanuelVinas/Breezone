package com.breezone.app.widgets

import android.appwidget.AppWidgetManager
import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.glance.appwidget.GlanceAppWidgetManager
import androidx.lifecycle.lifecycleScope
import com.breezone.app.shared.R
import com.breezone.app.ui.theme.BreezoneTheme
import kotlinx.coroutines.launch

/** A choice for a widget: what it shows, and where it comes from. */
private data class Choice(val title: String, val sub: String, val config: WidgetConfig)

/** Chooses what a widget shows, when it is placed on the home screen. */
class WidgetConfigActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val appWidgetId = intent?.extras?.getInt(AppWidgetManager.EXTRA_APPWIDGET_ID, AppWidgetManager.INVALID_APPWIDGET_ID) ?: AppWidgetManager.INVALID_APPWIDGET_ID
        // Leaving without a choice removes the widget.
        setResult(RESULT_CANCELED, Intent().putExtra(AppWidgetManager.EXTRA_APPWIDGET_ID, appWidgetId))
        val kind = kindOf(this, appWidgetId)
        if (appWidgetId == AppWidgetManager.INVALID_APPWIDGET_ID || kind == null) {
            finish()
            return
        }
        val pick = { c: WidgetConfig ->
            WidgetStore.set(this, appWidgetId, c)
            lifecycleScope.launch {
                runCatching { widgetOf(kind).refresh(this@WidgetConfigActivity, GlanceAppWidgetManager(this@WidgetConfigActivity).getGlanceIdBy(appWidgetId)) }
                setResult(RESULT_OK, Intent().putExtra(AppWidgetManager.EXTRA_APPWIDGET_ID, appWidgetId))
                finish()
            }
            Unit
        }
        setContent {
            BreezoneTheme {
                Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
                    var choices by remember { mutableStateOf<List<Choice>?>(null) }
                    LaunchedEffect(Unit) {
                        val found = choicesFor(kind)
                        // One server and nothing else to choose: done.
                        if (found.size == 1 && kind in setOf(WidgetKind.Energy, WidgetKind.Scenes)) pick(found.single().config) else choices = found
                    }
                    Column(Modifier.safeDrawingPadding().padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                        Text(stringResource(R.string.widget_config_title), style = MaterialTheme.typography.headlineSmall)
                        Text(stringResource(R.string.widget_config_hint), color = MaterialTheme.colorScheme.onSurfaceVariant)
                        when (val list = choices) {
                            null -> CircularProgressIndicator()
                            else -> if (list.isEmpty()) {
                                Text(stringResource(if (hub().servers.value.isEmpty()) R.string.widget_sign_in else R.string.widget_config_none))
                            } else {
                                LazyColumn(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                                    items(list, key = { it.config.toString() }) { c ->
                                        Column(Modifier.fillMaxWidth().clickable { pick(c.config) }.padding(vertical = 10.dp)) {
                                            Text(c.title, style = MaterialTheme.typography.bodyLarge)
                                            if (c.sub.isNotBlank()) Text(c.sub, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /** What a widget of this kind may show, on every server the app is signed in to. */
    private suspend fun choicesFor(kind: WidgetKind): List<Choice> = hub().servers.value.flatMap { server ->
        val id = server.account.id
        val api = api(id) ?: return@flatMap emptyList()
        val label = server.account.label.ifBlank { api.base }
        runCatching {
            when (kind) {
                WidgetKind.Energy, WidgetKind.Scenes -> listOf(Choice(label, api.base, WidgetConfig(id, title = label)))
                WidgetKind.Price -> api.status().map { i -> Choice(i.name, label, WidgetConfig(id, installationId = i.id, title = i.name)) }
                WidgetKind.Zone -> api.status().filter { it.controllable }.flatMap { i ->
                    i.zones.sortedBy { it.position }.map { z -> Choice(z.name, "${i.name} · $label", WidgetConfig(id, installationId = i.id, systemId = z.systemId, zoneId = z.zoneId, title = z.name)) }
                }
                WidgetKind.Device -> api.devices().filter { it.sourceId != null && it.widgetTrait() != null }.sortedBy { it.name }
                    .map { d -> Choice(d.name, label, WidgetConfig(id, deviceId = d.id, title = d.name)) }
            }
        }.getOrDefault(emptyList())
    }
}
