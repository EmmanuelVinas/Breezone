package com.breezone.app.ui.screens

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.Checkbox
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.breezone.app.data.Zone
import com.breezone.app.resources.Res
import com.breezone.app.resources.common_delete
import com.breezone.app.resources.common_save
import com.breezone.app.resources.routine_editor_delete_confirm
import com.breezone.app.resources.routine_editor_less
import com.breezone.app.resources.routine_editor_more
import com.breezone.app.resources.routine_editor_name
import com.breezone.app.resources.routine_editor_setpoint
import com.breezone.app.resources.routine_editor_zones
import com.breezone.app.ui.SetpointLimits
import com.breezone.app.ui.formatTemp
import com.breezone.app.ui.icons.FzIcons
import com.breezone.app.ui.theme.Fz
import org.jetbrains.compose.resources.stringResource

/**
 * List of a section of an installation (routines, schedules): a hint on top, the items, and the
 * "+" button when [onAdd] is given, with room under the last item for the button.
 */
@Composable
fun SectionList(
    hint: String,
    addLabel: String,
    onAdd: (() -> Unit)?,
    modifier: Modifier = Modifier,
    content: LazyListScope.() -> Unit,
) {
    Box(modifier.fillMaxSize()) {
        LazyColumn(
            Modifier.fillMaxSize(),
            contentPadding = SectionListPadding,
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            item(key = "hint", contentType = "hint") { Muted(hint) }
            content()
            item(key = "bottom-space", contentType = "space") { Spacer(Modifier.height(80.dp)) }
        }
        if (onAdd != null) {
            FloatingActionButton(
                onClick = onAdd,
                modifier = Modifier.align(Alignment.BottomEnd).padding(20.dp),
                containerColor = MaterialTheme.colorScheme.primary,
            ) { Icon(FzIcons.Plus, addLabel) }
        }
    }
}

/** Top row of an item card: colored icon, title, an optional line under it, and a trailing control. */
@Composable
fun ItemCardHeader(
    icon: ImageVector,
    tint: Color,
    title: String,
    subtitle: String? = null,
    trailing: @Composable () -> Unit = {},
) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Icon(icon, null, Modifier.size(18.dp), tint = tint)
        Spacer(Modifier.size(8.dp))
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.titleSmall)
            if (subtitle != null) Caption(subtitle)
        }
        trailing()
    }
}

/**
 * Bottom sheet of an editor: [title], the fields, then Delete (when [deleteName] is given, after
 * a confirmation) and Save.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun EditorSheet(
    title: String,
    canSave: Boolean,
    onSave: () -> Unit,
    onDismiss: () -> Unit,
    deleteName: String? = null,
    onDelete: () -> Unit = {},
    content: @Composable ColumnScope.() -> Unit,
) {
    val sheet = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    var confirmDelete by remember { mutableStateOf(false) }

    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = sheet) {
        Column(
            Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).padding(horizontal = 20.dp).navigationBarsPadding(),
            verticalArrangement = Arrangement.spacedBy(14.dp),
        ) {
            Text(title, style = MaterialTheme.typography.titleLarge)
            content()
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                if (deleteName != null) {
                    IconTextButton(FzIcons.Trash2, stringResource(Res.string.common_delete), { confirmDelete = true }, MaterialTheme.colorScheme.error)
                }
                Spacer(Modifier.weight(1f))
                Button(onClick = onSave, enabled = canSave, shape = Fz.shapes.button) { Text(stringResource(Res.string.common_save)) }
            }
            Spacer(Modifier.height(16.dp))
        }
    }

    if (confirmDelete && deleteName != null) {
        ConfirmDialog(
            title = stringResource(Res.string.routine_editor_delete_confirm, deleteName),
            confirm = stringResource(Res.string.common_delete),
            onConfirm = {
                confirmDelete = false
                onDelete()
            },
            onDismiss = { confirmDelete = false },
        )
    }
}

@Composable
fun NameField(value: String, onValueChange: (String) -> Unit, placeholder: String) {
    OutlinedTextField(
        value,
        onValueChange,
        label = { Text(stringResource(Res.string.routine_editor_name)) },
        placeholder = { Text(placeholder) },
        singleLine = true,
        modifier = Modifier.fillMaxWidth(),
        shape = Fz.shapes.field,
    )
}

@Composable
fun SetpointStepper(value: Double, onChange: (Double) -> Unit, limits: SetpointLimits) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Text(stringResource(Res.string.routine_editor_setpoint), Modifier.weight(1f))
        IconButton(onClick = { onChange(limits.clamp(value - limits.step)) }) {
            Icon(FzIcons.Minus, stringResource(Res.string.routine_editor_less))
        }
        Text(formatTemp(limits.clamp(value)), fontSize = 18.sp)
        IconButton(onClick = { onChange(limits.clamp(value + limits.step)) }) {
            Icon(FzIcons.Plus, stringResource(Res.string.routine_editor_more))
        }
    }
}

/** "Zones" title and one checkbox per zone ("group · name"); [keyOf] identifies a zone in [selected]. */
@Composable
fun <K> ZoneChecklist(zones: List<Zone>, selected: Set<K>, keyOf: (Zone) -> K, onChange: (Set<K>) -> Unit, note: String? = null) {
    Text(stringResource(Res.string.routine_editor_zones), style = MaterialTheme.typography.titleSmall)
    if (note != null) Muted(note)
    zones.forEach { z ->
        val key = keyOf(z)
        val checked = key in selected
        Row(
            Modifier.fillMaxWidth().clickable { onChange(if (checked) selected - key else selected + key) },
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Checkbox(checked = checked, onCheckedChange = { onChange(if (it) selected + key else selected - key) })
            Text(listOfNotNull(z.group, z.name).joinToString(" · "))
        }
    }
}
