package com.breezone.app.ui.home

import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.SliderDefaults
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.breezone.app.data.CatalogField
import com.breezone.app.data.CatalogTrait
import com.breezone.app.data.CatalogWidget
import com.breezone.app.data.DeviceView
import com.breezone.app.data.traitOf
import com.breezone.app.resources.Res
import com.breezone.app.resources.common_minus
import com.breezone.app.resources.common_no
import com.breezone.app.resources.common_plus
import com.breezone.app.resources.common_yes
import com.breezone.app.resources.device_unavailable
import com.breezone.app.ui.components.FzSlider
import com.breezone.app.ui.decimalSeparator
import com.breezone.app.ui.icons.FzIcons
import com.breezone.app.ui.theme.Fz
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.doubleOrNull
import org.jetbrains.compose.resources.stringResource
import kotlin.math.roundToInt

/** The widgets this version of the app knows; any other shows its field as a value. */
private val KNOWN = setOf("value", "toggle", "slider", "stepper", "gauge", "enum", "button")

private data class Piece(val trait: CatalogTrait, val widget: CatalogWidget, val field: CatalogField?)

/** The pieces of a device's card from its traits' descriptors (`ui.card`); pictures and charts left to a detail. */
private fun pieces(d: DeviceView, catalog: List<CatalogTrait>): List<Piece> = d.traits.flatMap { id ->
    val trait = catalog.traitOf(id) ?: return@flatMap emptyList()
    trait.ui?.card.orEmpty().filter { it.widget != "picture" && it.widget != "chart" }.map { w -> Piece(trait, w, trait.state.firstOrNull { it.name == w.field }) }
}

/** A value of a field in words: an enum's label, a number and its unit. */
@Composable
private fun fieldText(field: CatalogField, value: JsonElement?): String? {
    val p = value as? JsonPrimitive ?: return null
    return when (field.type) {
        "bool" -> p.booleanOrNull?.let { stringResource(if (it) Res.string.common_yes else Res.string.common_no) }
        "enum" -> p.contentOrNull?.let { field.valueLabels[it]?.text() ?: it }
        "integer", "number" -> p.doubleOrNull?.let { v ->
            val n = if (field.type == "integer") v.roundToInt().toString() else ((v * 10).roundToInt() / 10.0).toString().replace('.', decimalSeparator())
            listOfNotNull(n, field.unit).joinToString(" ")
        }
        else -> p.contentOrNull
    }
}

/**
 * A device of a kind this version of the app does not know (a module installed from a library may
 * bring one): its name, what sums it up, and its controls as its traits describe them.
 */
@Composable
fun GenericEntry(d: DeviceView, ctx: HomeContext) {
    val catalog by produceState<List<CatalogTrait>?>(null, ctx.serverId) { value = ctx.hub.catalog(ctx.serverId) }
    val list = catalog.orEmpty()
    val state = d.state ?: JsonObject(emptyMap())
    val primary = d.traits.firstNotNullOfOrNull { id ->
        val t = list.traitOf(id) ?: return@firstNotNullOfOrNull null
        val field = t.state.firstOrNull { it.name == t.ui?.primary } ?: return@firstNotNullOfOrNull null
        fieldText(field, state[field.name])
    }
    val kind = DeviceKind.Other
    Column {
        DeviceLine(kind.icon, kind.color(), false, d.name, if (!d.available) stringResource(Res.string.device_unavailable) else primary, onClick = null)
        val shown = pieces(d, list)
        if (shown.isNotEmpty()) {
            Column(Modifier.padding(start = 52.dp, bottom = 8.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                shown.forEach { PieceControl(d, it, state, ctx) }
            }
        }
    }
}

@Composable
private fun PieceControl(d: DeviceView, piece: Piece, state: JsonObject, ctx: HomeContext) {
    val (trait, w, field) = piece
    val value = w.field?.let { state[it] }
    val name = field?.label?.text()?.ifEmpty { null } ?: w.field ?: w.action.orEmpty()
    val enabled = d.usable
    val send: (JsonElement) -> Unit = { v ->
        val params = if (w.param != null) JsonObject(mapOf(w.param to v)) else (w.params ?: JsonObject(emptyMap()))
        val optimistic = if (w.param != null && w.field != null) JsonObject(mapOf(w.field to v)) else null
        ctx.hub.act(ctx.serverId, d, trait.trait, w.action ?: "set", params, optimistic)
    }
    val kind = if (w.widget in KNOWN && w.action != null) w.widget else "value"
    when (kind) {
        "toggle" -> Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Text(name, fontSize = 13.sp, color = Fz.colors.muted, modifier = Modifier.weight(1f))
            Switch(checked = (value as? JsonPrimitive)?.booleanOrNull == true, enabled = enabled, onCheckedChange = { send(JsonPrimitive(it)) })
        }
        "slider" -> {
            val min = (field?.min ?: 0.0).toFloat()
            val max = (field?.max ?: 100.0).toFloat()
            val current = ((value as? JsonPrimitive)?.doubleOrNull ?: min.toDouble()).toFloat()
            Column {
                Row {
                    Text(name, fontSize = 13.sp, color = Fz.colors.muted, modifier = Modifier.weight(1f))
                    if (field != null) Text(fieldText(field, value).orEmpty(), fontSize = 13.sp)
                }
                FzSlider(
                    value = current,
                    onCommit = { v -> send(if (field?.type == "integer") JsonPrimitive(v.roundToInt()) else JsonPrimitive(v.toDouble())) },
                    range = min..max,
                    colors = SliderDefaults.colors(),
                    enabled = enabled,
                )
            }
        }
        "stepper" -> {
            val step = if (field?.type == "integer") 1.0 else 0.5
            val current = (value as? JsonPrimitive)?.doubleOrNull ?: field?.min ?: 0.0
            val clamp = { v: Double -> v.coerceIn(field?.min ?: Double.NEGATIVE_INFINITY, field?.max ?: Double.POSITIVE_INFINITY) }
            val emit = { v: Double -> send(if (field?.type == "integer") JsonPrimitive(v.roundToInt()) else JsonPrimitive(v)) }
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Text(name, fontSize = 13.sp, color = Fz.colors.muted, modifier = Modifier.weight(1f))
                IconButton(onClick = { emit(clamp(current - step)) }, enabled = enabled) { Icon(FzIcons.Minus, stringResource(Res.string.common_minus), Modifier.size(18.dp)) }
                Text(field?.let { fieldText(it, value) } ?: current.toString(), fontWeight = FontWeight.Medium)
                IconButton(onClick = { emit(clamp(current + step)) }, enabled = enabled) { Icon(FzIcons.Plus, stringResource(Res.string.common_plus), Modifier.size(18.dp)) }
            }
        }
        "enum" -> Column {
            Text(name, fontSize = 13.sp, color = Fz.colors.muted)
            Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                val current = (value as? JsonPrimitive)?.contentOrNull
                field?.values.orEmpty().forEach { v ->
                    FilterChip(selected = v == current, enabled = enabled, onClick = { send(JsonPrimitive(v)) }, label = { Text(field?.valueLabels?.get(v)?.text() ?: v) })
                }
            }
        }
        "button" -> {
            val shownWhen = w.`when`?.let { (state[it] as? JsonPrimitive)?.booleanOrNull == true } ?: true
            if (shownWhen) {
                val label = trait.actions.firstOrNull { it.name == w.action }?.label?.text()?.ifEmpty { null } ?: w.action.orEmpty()
                OutlinedButton(onClick = { ctx.hub.act(ctx.serverId, d, trait.trait, w.action ?: "set", w.params ?: JsonObject(emptyMap())) }, enabled = enabled) { Text(label) }
            }
        }
        else -> {
            val text = field?.let { fieldText(it, value) } ?: return
            Row(Modifier.fillMaxWidth()) {
                Text(name, fontSize = 13.sp, color = Fz.colors.muted, modifier = Modifier.weight(1f))
                Text(text, fontSize = if (w.widget == "gauge") 20.sp else 13.sp, fontWeight = FontWeight.Medium)
            }
        }
    }
}
