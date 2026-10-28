package com.hivepaas.shellwing.ui

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.LibraryBooks
import androidx.compose.material.icons.filled.Add
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExposedDropdownMenuAnchorType
import androidx.compose.material3.ExposedDropdownMenuBox
import androidx.compose.material3.ExposedDropdownMenuDefaults
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.PrimaryScrollableTabRow
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.hivepaas.shellwing.AppModel
import com.hivepaas.shellwing.core.ConfigFileRow
import com.hivepaas.shellwing.core.EnvRow
import com.hivepaas.shellwing.core.NoteRow
import com.hivepaas.shellwing.core.SnippetEdit
import com.hivepaas.shellwing.core.SnippetRow
import com.hivepaas.shellwing.tr

/** The Library tab: what the vault keeps besides passwords, as the desktop's Library has it. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun LibraryScreen(model: AppModel, onOpen: (Screen) -> Unit) {
    var part by rememberSaveable { mutableIntStateOf(0) }
    val parts = listOf(tr("Snippets"), tr("Envs"), tr("Notes"), tr("Files"), tr("Keys"))
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(tr("Library")) },
                actions = {
                    if (part == 0) {
                        IconButton(onClick = { onOpen(Screen.SnippetLibrary) }) {
                            Icon(Icons.AutoMirrored.Outlined.LibraryBooks, tr("Snippet library"))
                        }
                    }
                },
            )
        },
        floatingActionButton = {
            val (label, open) = when (part) {
                0 -> tr("New snippet") to { onOpen(Screen.SnippetEditor(newSnippet())) }
                1 -> tr("New environment") to { onOpen(Screen.EnvEditor(EnvRow("", "", "", emptyList()))) }
                2 -> tr("New note") to { onOpen(Screen.NoteEditor(newNote())) }
                3 -> tr("New config file") to { onOpen(Screen.ConfigFileEditor(newConfigFile())) }
                else -> null to {}
            }
            if (label != null) {
                ExtendedFloatingActionButton(
                    onClick = open, icon = { Icon(Icons.Filled.Add, null) }, text = { Text(label) },
                    containerColor = Dusk.accent, contentColor = Dusk.onAccent,
                )
            }
        },
    ) { pad ->
        Column(Modifier.fillMaxSize().padding(pad)) {
            PrimaryScrollableTabRow(selectedTabIndex = part, edgePadding = 8.dp) {
                parts.forEachIndexed { i, title ->
                    Tab(selected = part == i, onClick = { part = i }, text = { Text(title) })
                }
            }
            Box(Modifier.weight(1f)) {
                when (part) {
                    0 -> SnippetsList(model, spec = null, onPick = { onOpen(Screen.SnippetEditor(it.edit())) })
                    1 -> EnvironmentsList(model) { onOpen(Screen.EnvEditor(it)) }
                    2 -> NotesList(model, spec = null) { onOpen(Screen.NoteEditor(it)) }
                    3 -> ConfigFilesList(model) { onOpen(Screen.ConfigFileEditor(it)) }
                    else -> KeysList(model)
                }
            }
        }
    }
}

fun newSnippet() = SnippetEdit("", "", "", "", true, "", emptyList())
fun newNote() = NoteRow("", "", "", true, false, 0u, emptyList(), false)
fun newConfigFile() = ConfigFileRow("", "", "", "~/", "600".toUInt(8), 0u, false, false, "")

fun SnippetRow.edit() = SnippetEdit(id, name, category, command, enter, scope, needs)

/** What a delete says: a synced vault forgets it on the other devices too. */
fun goesEverywhere() = tr("If the vault syncs, it goes from your other devices too.")

// Snippets

/**
 * Snippets by category, filtered. [onPick] edits one in the Library, runs
 * it in a session.
 */
@Composable
fun SnippetsList(model: AppModel, spec: String?, onPick: (SnippetRow) -> Unit) {
    var snippets by remember { mutableStateOf(model.core?.snippets(spec) ?: emptyList()) }
    var filter by rememberSaveable { mutableStateOf("") }
    val q = filter.lowercase()
    val shown = snippets.filter { q.isEmpty() || it.name.lowercase().contains(q) || it.command.lowercase().contains(q) }
    Library(
        filter = filter, onFilter = { filter = it }, filterHint = tr("Filter snippets"),
        empty = if (snippets.isEmpty()) {
            if (spec == null) tr("No snippets yet. Make one with +, or sync those of your other devices.")
            else tr("No snippets for this host.")
        } else null,
    ) {
        for ((cat, group) in shown.groupBy { it.category }) {
            item(key = "cat:$cat") { Heading(cat.ifEmpty { tr("No category") }, Modifier.padding(start = 16.dp, top = 12.dp)) }
            items(group, key = { it.id }) { s ->
                Item(
                    s.name, s.oneLine, mono = true,
                    onClick = { onPick(s) },
                    onDelete = if (spec == null) {
                        { model.core?.forget(s.id); snippets = model.core?.snippets(spec) ?: emptyList() }
                    } else null,
                    deleteTitle = tr("Delete {name}?", "name" to s.name),
                )
            }
        }
    }
}

// Environments

@Composable
fun EnvironmentsList(model: AppModel, onPick: (EnvRow) -> Unit) {
    var envs by remember { mutableStateOf(model.core?.environments() ?: emptyList()) }
    Library(
        empty = if (envs.isEmpty()) {
            tr("No environments yet: variables kept in the vault, loaded into a session's shell without showing their values.")
        } else null,
    ) {
        items(envs, key = { it.id }) { e ->
            Item(
                e.name, e.names.joinToString(" "), mono = true,
                onClick = { onPick(e) },
                onDelete = { model.core?.forget(e.id); envs = model.core?.environments() ?: emptyList() },
                deleteTitle = tr("Delete {name}?", "name" to e.name),
            )
        }
    }
}

// The pieces the lists share.

/** A list with, above it, a filter when one is given, and a word when it is empty. */
@Composable
fun Library(
    filter: String? = null,
    onFilter: (String) -> Unit = {},
    filterHint: String = "",
    empty: String? = null,
    content: LazyListScope.() -> Unit,
) {
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 96.dp)) {
        if (filter != null) {
            item {
                OutlinedTextField(
                    value = filter, onValueChange = onFilter, placeholder = { Text(filterHint) }, singleLine = true,
                    modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
                )
            }
        }
        if (empty != null) item { Text(empty, color = Dusk.muted, modifier = Modifier.padding(16.dp)) }
        content()
    }
}

/** A row of a list: a title and a line under it; a long press offers Delete, asked about first. */
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun Item(
    title: String,
    sub: String,
    mono: Boolean = false,
    onClick: () -> Unit,
    onDelete: (() -> Unit)? = null,
    deleteTitle: String = "",
    deleteSays: String = goesEverywhere(),
    deleteLabel: String = tr("Delete"),
    trailing: @Composable () -> Unit = {},
) {
    var menu by remember { mutableStateOf(false) }
    var asking by remember { mutableStateOf(false) }
    Box {
        androidx.compose.foundation.layout.Row(
            Modifier.fillMaxWidth()
                .combinedClickable(onClick = onClick, onLongClick = if (onDelete != null) ({ menu = true }) else null)
                .padding(horizontal = 16.dp, vertical = 12.dp),
            verticalAlignment = androidx.compose.ui.Alignment.CenterVertically,
        ) {
            Column(Modifier.weight(1f)) {
                Text(title, fontSize = 16.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                if (sub.isNotEmpty()) {
                    Text(
                        sub, fontSize = if (mono) 12.sp else 13.sp, color = if (mono) Dusk.soft else Dusk.muted,
                        fontFamily = if (mono) FontFamily.Monospace else FontFamily.Default,
                        maxLines = 2, overflow = TextOverflow.Ellipsis,
                    )
                }
            }
            trailing()
        }
        DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
            DropdownMenuItem(text = { Text(deleteLabel, color = Dusk.bad) }, onClick = { menu = false; asking = true })
        }
    }
    if (asking && onDelete != null) {
        AlertDialog(
            onDismissRequest = { asking = false },
            title = { Text(deleteTitle) },
            text = { Text(deleteSays) },
            confirmButton = {
                TextButton(onClick = { asking = false; onDelete() }) { Text(deleteLabel, color = Dusk.bad) }
            },
            dismissButton = { TextButton(onClick = { asking = false }) { Text(tr("Cancel")) } },
        )
    }
}

/** Where a snippet, an environment or a config file is offered: everywhere, a group's sessions, one host's. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ScopePicker(model: AppModel, scope: String, onChange: (String) -> Unit) {
    var open by remember { mutableStateOf(false) }
    val hosts = model.sections.flatMap { it.hosts }.map { it.name }.toSortedSet().toList()
    val options = buildList {
        add("" to tr("Everywhere"))
        model.groups().forEach { add("group:$it" to tr("Group: {group}", "group" to it)) }
        hosts.forEach { add("host:$it" to tr("Host: {host}", "host" to it)) }
        // One no longer here (synced, or renamed): kept as it is.
        if (scope.isNotEmpty() && none { it.first == scope }) add(scope to scope)
    }
    ExposedDropdownMenuBox(expanded = open, onExpandedChange = { open = it }) {
        OutlinedTextField(
            value = options.firstOrNull { it.first == scope }?.second ?: scope, onValueChange = {}, readOnly = true,
            label = { Text(tr("Offered")) }, trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(open) },
            modifier = Modifier.fillMaxWidth().menuAnchor(ExposedDropdownMenuAnchorType.PrimaryNotEditable),
        )
        ExposedDropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            for ((value, label) in options) {
                DropdownMenuItem(text = { Text(label) }, onClick = { onChange(value); open = false })
            }
        }
    }
}

@Composable
fun Gap(height: Int = 8) = Spacer(Modifier.height(height.dp))
