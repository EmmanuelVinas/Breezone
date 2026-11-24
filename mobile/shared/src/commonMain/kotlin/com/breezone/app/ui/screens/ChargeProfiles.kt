package com.breezone.app.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Button
import androidx.compose.material3.FilterChip
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
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
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.breezone.app.data.ChargeBar
import com.breezone.app.data.ChargeProfileRef
import com.breezone.app.data.ChargeProfileView
import com.breezone.app.data.ChargeProfileWrite
import com.breezone.app.data.ChargeRulePlan
import com.breezone.app.data.Hub
import com.breezone.app.resources.*
import com.breezone.app.ui.decimalSeparator
import com.breezone.app.ui.icons.FzIcons
import com.breezone.app.ui.theme.Fz
import kotlinx.coroutines.launch
import org.jetbrains.compose.resources.getString
import org.jetbrains.compose.resources.stringResource
import org.koin.compose.koinInject
import kotlin.math.roundToInt

private val COLORS = listOf("#3b82f6", "#e5e7eb", "#ef4444", "#14b8a6", "#f59e0b", "#a855f7")

private fun kwText(kw: Double?) = kw?.let { ((it * 10).roundToInt() / 10.0).toString().removeSuffix(".0").replace('.', decimalSeparator()) }.orEmpty()

/** A profile being written, with the id it was saved under (`null`: new). */
private data class Draft(val id: Long?, val key: String, val write: ChargeProfileWrite)

private fun draftsOf(list: List<ChargeProfileView>) = list.map { Draft(it.id, "p${it.id}", ChargeProfileWrite(it.name, it.kw, it.color)) }

/** An automation in one sentence, from its summary. */
@Composable
private fun sentence(r: ChargeRulePlan): String {
    val s = r.summary.copy(`when` = r.summary.`when`.replace(Regex("^(quand|when|cuando)\\s+", RegexOption.IGNORE_CASE), ""))
    val then = s.actions.joinToString(", ")
    val head = if (s.conditions.isEmpty()) stringResource(Res.string.charge_rule_when, s.`when`, then)
    else stringResource(Res.string.charge_rule_when_if, s.`when`, s.conditions.joinToString(" ${s.joined} "), then)
    val parts = listOfNotNull(
        head,
        s.otherwise.takeIf { it.isNotEmpty() }?.let { stringResource(Res.string.charge_rule_else, it.joinToString(", ")) },
        s.over.takeIf { it.isNotEmpty() }?.let { stringResource(Res.string.charge_rule_over, it.joinToString(", ")) },
    )
    return parts.joinToString(" ; ") + "."
}

/** A frieze of an automation: a day of 24 hours, read only. */
@Composable
private fun RuleBar(bar: ChargeBar, profiles: List<ChargeProfileRef>) {
    Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
        Row(
            Modifier.fillMaxWidth().height(28.dp).clip(RoundedCornerShape(8.dp))
                .background(MaterialTheme.colorScheme.background)
                .border(1.dp, MaterialTheme.colorScheme.outlineVariant, RoundedCornerShape(8.dp)),
        ) {
            bar.segments.forEach { s ->
                Box(
                    Modifier.weight((s.end - s.start).coerceAtLeast(1).toFloat()).fillMaxHeight().background(effectColor(s.effect, profiles)).padding(horizontal = 6.dp),
                    contentAlignment = Alignment.CenterStart,
                ) {
                    if (s.end - s.start >= 180) {
                        Text(
                            effectText(s.effect, profiles),
                            fontSize = 11.sp,
                            fontWeight = FontWeight.SemiBold,
                            maxLines = 1,
                            color = if (dim(s.effect)) Fz.colors.muted else MaterialTheme.colorScheme.onPrimary,
                        )
                    }
                }
            }
        }
        Row {
            listOf("0 h", "6 h", "12 h", "18 h", "24 h").forEachIndexed { i, h ->
                if (i > 0) Spacer(Modifier.weight(1f))
                Text(h, fontSize = 10.sp, color = Fz.colors.muted)
            }
        }
    }
}

@Composable
private fun RuleCard(r: ChargeRulePlan, profiles: List<ChargeProfileRef>) {
    SectionCard(Modifier.alpha(if (!r.enabled || r.paused) 0.6f else 1f)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(r.name, fontWeight = FontWeight.SemiBold, modifier = Modifier.weight(1f))
            Caption(
                stringResource(
                    when {
                        r.paused -> Res.string.charge_profiles_paused
                        r.enabled -> Res.string.charge_profiles_active
                        else -> Res.string.charge_profiles_disabled
                    },
                ),
            )
        }
        Caption(sentence(r))
        val several = r.devices.size > 1
        r.bars.forEach { b ->
            Row(verticalAlignment = Alignment.CenterVertically) {
                hexColor(b.color)?.let {
                    Box(Modifier.size(8.dp).background(it, CircleShape))
                    Spacer(Modifier.width(6.dp))
                }
                val label = when {
                    b.others -> stringResource(Res.string.charge_profiles_other_days)
                    b.label.isNotBlank() -> b.label
                    else -> stringResource(Res.string.charge_profiles_every_day)
                }
                Text(if (several) "$label · ${r.devices.firstOrNull { it.id == b.deviceId }?.name.orEmpty()}" else label, fontSize = 13.sp)
            }
            RuleBar(b, profiles)
        }
        if (r.assumed) Caption(stringResource(Res.string.charge_profiles_assumed))
    }
}

/** Recharge › Profiles: the powers a charger may follow, and what the automations plan. */
@Composable
fun ChargeProfilesScreen(serverId: String, onBack: () -> Unit) {
    val hub = koinInject<Hub>()
    val scope = rememberCoroutineScope()
    val admin = hub.api(serverId)?.user?.isAdmin == true
    var list by remember { mutableStateOf<List<ChargeProfileView>?>(null) }
    var rules by remember { mutableStateOf<List<ChargeRulePlan>>(emptyList()) }
    var drafts by remember { mutableStateOf<List<Draft>>(emptyList()) }
    var saving by remember { mutableStateOf(false) }
    val reload: suspend () -> Unit = {
        hub.attempt(serverId) { chargeProfiles() }?.let {
            list = it
            drafts = draftsOf(it)
        }
        hub.attempt(serverId) { chargeAutomations() }?.let { rules = it }
    }
    LaunchedEffect(serverId) { reload() }
    val saved = draftsOf(list.orEmpty())
    val issue = drafts.firstOrNull { it.write.name.isBlank() || (it.write.kw != null && (it.write.kw <= 0 || it.write.kw > 50)) }
    val refs = list.orEmpty().map { ChargeProfileRef(it.id, it.name, it.kw, it.color) }
    Scaffold(topBar = { FzTopBar(stringResource(Res.string.charge_profiles_title), stringResource(Res.string.house_charging), onBack) }) { padding ->
        LazyColumn(
            Modifier.fillMaxSize(),
            contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = padding.calculateTopPadding() + 4.dp, bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            item(key = "profiles", contentType = "profiles") {
                SectionCard {
                    SectionTitle(stringResource(Res.string.charge_profiles_list))
                    Caption(stringResource(Res.string.charge_profiles_hint))
                    if (drafts.isEmpty()) Muted(stringResource(Res.string.charge_profiles_none))
                    drafts.forEachIndexed { i, d ->
                        val set = { w: ChargeProfileWrite -> drafts = drafts.mapIndexed { j, o -> if (j == i) o.copy(write = w) else o } }
                        ProfileRow(d, list.orEmpty().firstOrNull { it.id == d.id }, admin, set) { drafts = drafts.filterIndexed { j, _ -> j != i } }
                    }
                    if (issue != null) {
                        Caption(
                            stringResource(if (issue.write.name.isBlank()) Res.string.charge_profiles_needs_name else Res.string.charge_profiles_power_range),
                            color = MaterialTheme.colorScheme.error,
                        )
                    }
                    if (admin) {
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            OutlinedButton(
                                onClick = {
                                    scope.launch {
                                        val name = getString(Res.string.charge_profiles_n, drafts.size + 1)
                                        drafts = drafts + Draft(null, "n${drafts.size}-${saved.size}", ChargeProfileWrite(name, 7.0, COLORS[(drafts.size + 3) % COLORS.size]))
                                    }
                                },
                                enabled = drafts.size < 20,
                                shape = Fz.shapes.button,
                            ) { Text(stringResource(Res.string.charge_profiles_add)) }
                            Button(
                                onClick = {
                                    saving = true
                                    scope.launch {
                                        saved.filter { s -> drafts.none { it.id == s.id } }.forEach { s -> hub.attempt(serverId) { deleteChargeProfile(s.id!!) } }
                                        drafts.forEach { d ->
                                            val before = saved.firstOrNull { it.id == d.id }
                                            val w = d.write.copy(name = d.write.name.trim())
                                            when {
                                                d.id == null -> hub.attempt(serverId) { createChargeProfile(w) }
                                                before?.write != d.write -> hub.attempt(serverId) { saveChargeProfile(d.id, w) }
                                            }
                                        }
                                        reload()
                                        saving = false
                                    }
                                },
                                enabled = !saving && issue == null && drafts.map { it.id to it.write } != saved.map { it.id to it.write },
                                shape = Fz.shapes.button,
                            ) { Text(stringResource(Res.string.common_save)) }
                        }
                    }
                    Caption(stringResource(Res.string.charge_profiles_color_hint))
                }
            }
            item(key = "rules-head", contentType = "head") {
                Column(Modifier.padding(top = 8.dp)) {
                    SectionTitle(stringResource(Res.string.charging_plan_title))
                    Caption(stringResource(Res.string.charge_profiles_rules_hint))
                }
            }
            if (rules.isEmpty()) {
                item(key = "rules-empty", contentType = "note") { SectionCard { Muted(stringResource(Res.string.charge_profiles_rules_empty)) } }
            }
            items(rules.filter { it.placed }, key = { "rule-${it.id}" }, contentType = { "rule" }) { r -> RuleCard(r, refs) }
            val loose = rules.filter { !it.placed }
            if (loose.isNotEmpty()) {
                item(key = "loose", contentType = "loose") {
                    SectionCard {
                        SectionTitle(stringResource(Res.string.charge_profiles_no_time))
                        loose.forEach { r ->
                            Column(Modifier.alpha(if (!r.enabled || r.paused) 0.6f else 1f)) {
                                Text(r.name, fontWeight = FontWeight.Medium)
                                Caption(sentence(r))
                            }
                        }
                    }
                }
            }
            if (rules.isNotEmpty()) {
                item(key = "legend", contentType = "legend") {
                    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                        EffectLegend(refs)
                        Caption(stringResource(Res.string.charge_profiles_last_wins))
                    }
                }
            }
        }
    }
}

/** A profile: its colour, its name, its power, the automations that give it. */
@Composable
private fun ProfileRow(d: Draft, view: ChargeProfileView?, admin: Boolean, onChange: (ChargeProfileWrite) -> Unit, onRemove: () -> Unit) {
    val w = d.write
    Column(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp)).background(MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.5f)).padding(10.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            OutlinedTextField(
                value = w.name,
                onValueChange = { onChange(w.copy(name = it)) },
                enabled = admin,
                singleLine = true,
                label = { Text(stringResource(Res.string.charge_profiles_name)) },
                modifier = Modifier.weight(1f),
            )
            if (admin) IconTextButton(FzIcons.Trash2, stringResource(Res.string.common_delete), onRemove, MaterialTheme.colorScheme.error)
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            COLORS.forEach { c ->
                val color = hexColor(c) ?: return@forEach
                Box(
                    Modifier.size(22.dp).clip(CircleShape).background(color)
                        .then(if (w.color == c) Modifier.border(2.dp, MaterialTheme.colorScheme.primary, CircleShape) else Modifier)
                        .then(if (admin) Modifier.clickable { onChange(w.copy(color = c)) } else Modifier),
                )
            }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            FilterChip(selected = w.kw != null, enabled = admin, onClick = { if (w.kw == null) onChange(w.copy(kw = view?.kw ?: 7.0)) }, label = { Text(stringResource(Res.string.charge_profiles_up_to)) })
            FilterChip(selected = w.kw == null, enabled = admin, onClick = { onChange(w.copy(kw = null)) }, label = { Text(stringResource(Res.string.charge_profiles_max)) })
            if (w.kw != null) {
                var text by remember(d.key) { mutableStateOf(kwText(w.kw)) }
                OutlinedTextField(
                    value = text,
                    onValueChange = { v ->
                        text = v
                        v.replace(',', '.').toDoubleOrNull()?.takeIf { it > 0 }?.let { onChange(w.copy(kw = it)) }
                    },
                    enabled = admin,
                    singleLine = true,
                    suffix = { Text(stringResource(Res.string.charging_kw_unit)) },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
                    modifier = Modifier.width(110.dp),
                )
            }
        }
        Caption(view?.automations?.takeIf { it.isNotEmpty() }?.joinToString(" · ") { it.name } ?: stringResource(Res.string.charge_profiles_no_rule))
    }
}
