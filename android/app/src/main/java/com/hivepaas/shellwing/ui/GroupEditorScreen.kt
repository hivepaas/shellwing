package com.hivepaas.shellwing.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.outlined.Block
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.hivepaas.shellwing.AppModel
import com.hivepaas.shellwing.core.GroupEdit
import com.hivepaas.shellwing.core.ThemeRow
import com.hivepaas.shellwing.core.hostColors
import com.hivepaas.shellwing.said
import com.hivepaas.shellwing.tr
import com.hivepaas.shellwing.trN

/**
 * A group: its name, its color, and the theme of its hosts, as the iOS
 * app's group editor has it; a new one when [name] is null. A host goes in
 * a group from its own editor.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun GroupEditorScreen(model: AppModel, name: String?, onDone: () -> Unit) {
    val oldName = name ?: ""
    var edit by remember {
        mutableStateOf(name?.let { model.core?.group(it) } ?: GroupEdit(id = null, name = oldName, color = null, theme = null))
    }
    var problem by remember { mutableStateOf<String?>(null) }
    var picking by remember { mutableStateOf(false) }
    var deleting by remember { mutableStateOf(false) }
    val themes = remember { model.core?.themes() ?: emptyList() }
    val members = model.sections.firstOrNull { !it.favorites && it.name == oldName }?.hosts?.size ?: 0

    if (picking) {
        BackHandler { picking = false }
        ThemePicker(themes, edit.theme, appDefault = true, onBack = { picking = false }) {
            edit = edit.copy(theme = it)
            picking = false
        }
        return
    }

    fun save() {
        try {
            model.core?.saveGroup(edit, name)
            model.reload()
            onDone()
        } catch (e: Exception) {
            problem = e.said()
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                navigationIcon = { IconButton(onClick = onDone) { Icon(Icons.Filled.Close, tr("Cancel")) } },
                title = { Text(name ?: tr("New group")) },
                actions = { TextButton(onClick = { save() }, enabled = edit.name.isNotBlank()) { Text(tr("Save")) } },
            )
        },
    ) { pad ->
        Column(
            Modifier.fillMaxSize().padding(pad).imePadding().verticalScroll(rememberScrollState())
                .padding(horizontal = 16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text(tr("Group"), color = Dusk.accent, fontSize = 14.sp, modifier = Modifier.padding(top = 8.dp))
            OutlinedTextField(
                value = edit.name, onValueChange = { edit = edit.copy(name = it) },
                label = { Text(tr("Name")) }, placeholder = { Text(tr("Production")) }, singleLine = true,
                keyboardOptions = KeyboardOptions(autoCorrectEnabled = false),
                modifier = Modifier.fillMaxWidth(),
            )
            ColorChoice(edit.color) { edit = edit.copy(color = it) }
            Column(
                Modifier.fillMaxWidth().heightIn(min = 56.dp).clip(RoundedCornerShape(8.dp)).clickable { picking = true }
                    .padding(vertical = 8.dp),
            ) {
                Text(tr("Theme for its hosts"), fontSize = 16.sp)
                Text(
                    edit.theme?.let { id -> themes.firstOrNull { it.id == id }?.name ?: id } ?: tr("Same as the app"),
                    fontSize = 14.sp, color = Dusk.muted,
                )
            }
            Text(
                tr("A host's own theme wins over its group's. Choose the group in a host's editor to put the host in it."),
                fontSize = 13.sp, color = Dusk.muted,
            )
            problem?.let { Text(it, color = Dusk.bad) }
            if (name != null) {
                TextButton(onClick = { deleting = true }) { Text(tr("Delete…"), color = Dusk.bad) }
            }
            Spacer(Modifier.height(32.dp))
        }
    }

    if (deleting && name != null) {
        AlertDialog(
            onDismissRequest = { deleting = false },
            title = {
                Text(
                    if (members == 0) tr("Delete {group}?", "group" to name)
                    else trN(members, "Delete {group}? Its {n} host stays, in no group.", "Delete {group}? Its {n} hosts stay, in no group.", "group" to name),
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    deleting = false
                    try {
                        model.core?.deleteGroup(name)
                        model.reload()
                        onDone()
                    } catch (e: Exception) {
                        problem = e.said()
                    }
                }) { Text(tr("Delete"), color = Dusk.bad) }
            },
            dismissButton = { TextButton(onClick = { deleting = false }) { Text(tr("Cancel")) } },
        )
    }
}

/** "Color" and its swatches: none, then the colors a host or a group can be marked with, by name. */
@Composable
fun ColorChoice(color: String?, onChange: (String?) -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Text(tr("Color"), fontSize = 16.sp)
        Row(horizontalArrangement = Arrangement.spacedBy(14.dp)) {
            Swatch(null, Dusk.button, color == null) { onChange(null) }
            for (c in hostColors()) Swatch(c.name, rgb(c.rgb), color == c.name) { onChange(c.name) }
        }
    }
}

/** A color to mark with, ringed when chosen; [name] null is none. */
@Composable
private fun Swatch(name: String?, color: Color, on: Boolean, onClick: () -> Unit) {
    val said = name ?: tr("No color")
    Box(
        Modifier.size(36.dp).clip(CircleShape).border(2.dp, if (on) Dusk.text else Color.Transparent, CircleShape)
            .clickable(role = Role.RadioButton, onClick = onClick)
            .semantics {
                selected = on
                contentDescription = said
            },
        contentAlignment = Alignment.Center,
    ) {
        Box(Modifier.size(26.dp).clip(CircleShape).background(color), contentAlignment = Alignment.Center) {
            if (name == null) Icon(Icons.Outlined.Block, null, tint = Dusk.muted, modifier = Modifier.size(16.dp))
        }
    }
}

/**
 * The terminal's themes, each shown in its own colors; with [appDefault],
 * "Same as the app" first (null), for a host's or a group's.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ThemePicker(themes: List<ThemeRow>, selected: String?, appDefault: Boolean, onBack: () -> Unit, onPick: (String?) -> Unit) {
    Scaffold(
        topBar = {
            TopAppBar(
                navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, tr("Cancel")) } },
                title = { Text(tr("Theme")) },
            )
        },
    ) { pad ->
        LazyColumn(Modifier.fillMaxSize().padding(pad)) {
            if (appDefault) {
                item {
                    ThemeItem(selected == null, { onPick(null) }) { Text(tr("Same as the app"), fontSize = 16.sp) }
                }
            }
            items(themes, key = { it.id }) { t ->
                ThemeItem(selected == t.id, { onPick(t.id) }) {
                    Column(
                        Modifier.clip(RoundedCornerShape(6.dp)).background(rgb(t.background))
                            .border(1.dp, Dusk.line, RoundedCornerShape(6.dp)).padding(8.dp),
                        verticalArrangement = Arrangement.spacedBy(4.dp),
                    ) {
                        Text("~ $ ls", color = rgb(t.foreground), fontFamily = FontFamily.Monospace, fontSize = 12.sp)
                        Row(horizontalArrangement = Arrangement.spacedBy(2.dp)) {
                            for (c in t.palette) Box(Modifier.size(8.dp).clip(RoundedCornerShape(2.dp)).background(rgb(c)))
                        }
                    }
                    Spacer(Modifier.width(12.dp))
                    Text(t.name, fontSize = 16.sp)
                }
            }
        }
    }
}

@Composable
private fun ThemeItem(on: Boolean, onClick: () -> Unit, content: @Composable () -> Unit) {
    Row(
        Modifier.fillMaxWidth().heightIn(min = 56.dp).clickable(onClick = onClick)
            .semantics { selected = on }.padding(horizontal = 16.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Row(Modifier.weight(1f), verticalAlignment = Alignment.CenterVertically) { content() }
        if (on) Icon(Icons.Filled.Check, null, tint = Dusk.accent)
    }
}
