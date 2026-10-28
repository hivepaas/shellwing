package com.hivepaas.shellwing.ui

import android.content.Intent
import android.net.Uri
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.ArrowDropDown
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material.icons.outlined.Folder
import androidx.compose.material3.Button
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CheckboxDefaults
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.hivepaas.shellwing.AppModel
import com.hivepaas.shellwing.core.FileUnder
import com.hivepaas.shellwing.core.LibraryItem
import com.hivepaas.shellwing.core.LibraryPick
import com.hivepaas.shellwing.core.libraryCommand
import com.hivepaas.shellwing.core.libraryTasks
import com.hivepaas.shellwing.core.snippetAsked
import com.hivepaas.shellwing.said
import com.hivepaas.shellwing.tr
import com.hivepaas.shellwing.trN
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

private const val FILE_UNDER = "library.fileUnder"

private fun asItComes(index: UInt) = LibraryPick(index, emptyList(), false, null, null)

/**
 * The snippet library, as the desktop's library window and the iOS app's
 * SnippetLibraryView have it: ready commands by tool and task, to tick and
 * add to one's snippets, each adjusted first if need be (its options, on the
 * host or in a container, sudo, the environment that has its password).
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SnippetLibraryScreen(model: AppModel, onBack: () -> Unit) {
    val scope = rememberCoroutineScope()
    var items by remember { mutableStateOf(model.core?.libraryItems() ?: emptyList()) }
    var query by rememberSaveable { mutableStateOf("") }
    var tool by rememberSaveable { mutableStateOf<String?>(null) }
    var task by rememberSaveable { mutableStateOf<String?>(null) }
    var notAdded by rememberSaveable { mutableStateOf(false) }
    val checked = remember { mutableStateMapOf<UInt, Boolean>() }
    // How each is to be added, once changed from as it comes.
    val picks = remember { mutableStateMapOf<UInt, LibraryPick>() }
    var fileUnder by remember { mutableStateOf(model.prefs.getString(FILE_UNDER, "tool") ?: "tool") }
    var said by remember { mutableStateOf<String?>(null) }
    var problem by remember { mutableStateOf<String?>(null) }
    var showing by remember { mutableStateOf<UInt?>(null) }
    var fileMenu by remember { mutableStateOf(false) }

    val under = when (fileUnder) {
        "task" -> FileUnder.TASK
        "both" -> FileUnder.BOTH
        else -> FileUnder.TOOL
    }
    fun reload() {
        items = model.core?.libraryItems() ?: emptyList()
    }
    fun show(text: String) {
        said = text
        scope.launch {
            delay(4000)
            if (said == text) said = null
        }
    }
    fun add() {
        val chosen = checked.filterValues { it }.keys.sorted().map { picks[it] ?: asItComes(it) }
        try {
            val n = model.core?.addFromLibrary(chosen, under) ?: 0u
            checked.clear()
            problem = null
            show(trN(n.toInt(), "{n} snippet added: run it from a session's Snippets key.", "{n} snippets added: run them from a session's Snippets key."))
            reload()
        } catch (e: Exception) {
            problem = e.said()
        }
    }

    val open = showing?.let { n -> items.firstOrNull { it.index == n } }
    if (open != null) {
        LibraryItemScreen(
            model, open, under,
            pick = picks[open.index] ?: asItComes(open.index),
            onPick = { picks[open.index] = it },
            checked = checked[open.index] == true,
            onCheck = { checked[open.index] = it },
            onBack = { showing = null },
            onAdded = {
                showing = null
                reload()
            },
        )
        return
    }

    val words = query.lowercase().split(' ').filter { it.isNotEmpty() }
    val shown = items.filter { i ->
        (tool == null || i.tool == tool) && (task == null || i.task == task) && (!notAdded || i.added == 0u) &&
            words.all { w -> listOf(i.name, i.about, i.tool, i.task, i.id).any { it.lowercase().contains(w) } }
    }
    val count = checked.count { it.value }

    Scaffold(
        topBar = {
            TopAppBar(
                navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, tr("Done")) } },
                title = { Text(tr("Snippet library")) },
                actions = {
                    Box {
                        IconButton(onClick = { fileMenu = true }) { Icon(Icons.Outlined.Folder, tr("File under")) }
                        DropdownMenu(expanded = fileMenu, onDismissRequest = { fileMenu = false }) {
                            Text(tr("File under"), fontSize = 13.sp, color = Dusk.muted, modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp))
                            for ((value, label) in listOf(
                                "tool" to tr("Its tool (PostgreSQL)"),
                                "task" to tr("Its task (Backup)"),
                                "both" to tr("Both (PostgreSQL · Backup)"),
                            )) {
                                DropdownMenuItem(
                                    text = { Text(label) },
                                    trailingIcon = { if (fileUnder == value) Icon(Icons.Filled.Check, null) },
                                    onClick = {
                                        fileMenu = false
                                        fileUnder = value
                                        model.prefs.edit().putString(FILE_UNDER, value).apply()
                                    },
                                )
                            }
                        }
                    }
                },
            )
        },
        bottomBar = {
            val text = said ?: problem
            if (text != null) {
                Text(
                    text, fontSize = 13.sp, color = if (problem != null && said == null) Dusk.bad else Dusk.text,
                    modifier = Modifier.fillMaxWidth().background(Dusk.raised).navigationBarsPadding().padding(12.dp),
                )
            } else if (count > 0) {
                Row(
                    Modifier.fillMaxWidth().background(Dusk.raised).navigationBarsPadding().padding(horizontal = 16.dp, vertical = 10.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    TextButton(onClick = { checked.clear() }) { Text(tr("Clear"), color = Dusk.soft) }
                    Spacer(Modifier.weight(1f))
                    Button(onClick = ::add) { Text(trN(count, "Add {n} to my snippets", "Add {n} to my snippets")) }
                }
            }
        },
    ) { pad ->
        LazyColumn(Modifier.fillMaxSize().padding(pad), contentPadding = PaddingValues(bottom = 24.dp)) {
            item {
                Column(Modifier.padding(horizontal = 16.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    OutlinedTextField(
                        value = query, onValueChange = { query = it }, singleLine = true,
                        placeholder = { Text(tr("Find a command: a tool, a task, a word…")) },
                        modifier = Modifier.fillMaxWidth(),
                    )
                    Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        PickChip(tool ?: tr("Every tool"), tool != null, listOf<String?>(null) + items.map { it.tool }.distinct(), { it ?: tr("Every tool") }) { tool = it }
                        PickChip(task ?: tr("Every task"), task != null, listOf<String?>(null) + libraryTasks(), { it ?: tr("Every task") }) { task = it }
                        FilterChip(
                            selected = notAdded, onClick = { notAdded = !notAdded }, label = { Text(tr("Not added yet")) },
                            leadingIcon = if (notAdded) ({ Icon(Icons.Filled.Check, null, Modifier.size(18.dp)) }) else null,
                        )
                    }
                    Hint(tr("Tick the commands to add; each becomes a snippet of your own."))
                }
            }
            if (shown.isEmpty()) {
                item { Text(tr("Nothing here: try other words, or every task."), color = Dusk.muted, modifier = Modifier.padding(16.dp)) }
            }
            // By tool, in the library's order.
            for ((group, rows) in shown.groupBy { it.tool }) {
                item(key = "tool:$group") { Heading(group, Modifier.padding(start = 16.dp, top = 14.dp, bottom = 4.dp)) }
                items(rows, key = { it.index.toLong() }) { i ->
                    ItemRow(i, checked[i.index] == true, onCheck = { checked[i.index] = it }) { showing = i.index }
                }
            }
        }
    }
}

/** A filter of a few values: a chip whose tap opens the list. */
@Composable
private fun <T> PickChip(label: String, on: Boolean, values: List<T>, name: (T) -> String, onPick: (T) -> Unit) {
    var open by remember { mutableStateOf(false) }
    Box {
        FilterChip(
            selected = on, onClick = { open = true }, label = { Text(label) },
            trailingIcon = { Icon(Icons.Filled.ArrowDropDown, null, Modifier.size(18.dp)) },
        )
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            for (v in values) DropdownMenuItem(text = { Text(name(v)) }, onClick = { open = false; onPick(v) })
        }
    }
}

@Composable
private fun ItemRow(i: LibraryItem, checked: Boolean, onCheck: (Boolean) -> Unit, onOpen: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().clickable(onClick = onOpen).padding(start = 4.dp, end = 16.dp, top = 6.dp, bottom = 6.dp),
        verticalAlignment = Alignment.Top,
    ) {
        Checkbox(
            checked = checked, onCheckedChange = onCheck,
            colors = CheckboxDefaults.colors(checkedColor = Dusk.accent, checkmarkColor = Dusk.onAccent),
            modifier = Modifier.semantics { contentDescription = i.name },
        )
        Column(Modifier.weight(1f).padding(top = 10.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(i.name, fontSize = 15.sp, fontWeight = FontWeight.Medium)
                if (i.changesData) {
                    Spacer(Modifier.width(6.dp))
                    Icon(Icons.Filled.Warning, tr("Makes changes"), tint = Dusk.amber, modifier = Modifier.size(14.dp))
                }
            }
            Text(i.about, fontSize = 13.sp, color = Dusk.soft, maxLines = 2, overflow = TextOverflow.Ellipsis)
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                Badge(i.task, Dusk.muted)
                if (i.added > 0u) Badge(if (i.added == 1u) tr("Added") else tr("Added ×{n}", "n" to i.added.toString()), Dusk.ok)
            }
        }
    }
}

@Composable
private fun Badge(text: String, color: Color) {
    Text(
        text, fontSize = 11.sp, color = color,
        modifier = Modifier.clip(CircleShape).background(Dusk.chip).padding(horizontal = 6.dp, vertical = 1.dp),
    )
}

/** One ready command: what it runs, and how it is to be added. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun LibraryItemScreen(
    model: AppModel,
    item: LibraryItem,
    under: FileUnder,
    pick: LibraryPick,
    onPick: (LibraryPick) -> Unit,
    checked: Boolean,
    onCheck: (Boolean) -> Unit,
    onBack: () -> Unit,
    onAdded: () -> Unit,
) {
    val context = LocalContext.current
    var problem by remember { mutableStateOf<String?>(null) }
    val envs = remember { model.core?.environments() ?: emptyList() }
    BackHandler(onBack = onBack)
    val command = libraryCommand(pick)

    Scaffold(
        topBar = {
            TopAppBar(
                navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, tr("Snippet library")) } },
                title = { Text(item.name, maxLines = 1, overflow = TextOverflow.Ellipsis) },
            )
        },
    ) { pad ->
        Column(
            Modifier.fillMaxSize().padding(pad).verticalScroll(rememberScrollState()).padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Heading("${item.tool} · ${item.task}")
            Text(item.about, color = Dusk.soft)
            SelectionContainer {
                Text(
                    command, fontFamily = FontFamily.Monospace, fontSize = 13.sp,
                    modifier = Modifier.fillMaxWidth().clip(RoundedCornerShape(10.dp)).background(Dusk.field).padding(12.dp),
                )
            }
            item.docs?.let { docs ->
                TextButton(onClick = { runCatching { context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(docs))) } }) {
                    Text(tr("Docs ↗"))
                }
            }
            if (item.changesData) Hint(tr("It makes changes: added, it is typed and waits for you to press Enter."))
            val asked = snippetAsked(command)
            if (asked.isNotEmpty()) {
                Hint(tr("Asked when it runs: {names}. Once added, it is yours to edit.", "names" to asked.joinToString(", ")))
            }

            if (item.options.isNotEmpty()) {
                HorizontalDivider(color = Dusk.line)
                Heading(tr("Options"))
                item.options.forEachIndexed { n, option ->
                    val on = pick.options.contains(n.toUInt())
                    Row(
                        Modifier.fillMaxWidth().clickable {
                            onPick(pick.copy(options = (pick.options - n.toUInt()).let { if (on) it else (it + n.toUInt()).sorted() }))
                        },
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text(option, fontFamily = FontFamily.Monospace, fontSize = 14.sp, modifier = Modifier.weight(1f))
                        Switch(checked = on, onCheckedChange = null)
                    }
                }
            }

            HorizontalDivider(color = Dusk.line)
            if (item.canContain) {
                Heading(tr("Runs"))
                for ((inside, label) in listOf(false to tr("On the host"), true to tr("In a Docker container: asks which"))) {
                    Row(
                        Modifier.fillMaxWidth().clickable { onPick(pick.copy(container = inside)) },
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        RadioButton(selected = pick.container == inside, onClick = null)
                        Spacer(Modifier.width(12.dp))
                        Text(label)
                    }
                }
                if (pick.container) Hint(tr("The files it reads or writes are the container's."))
            }
            val sudo = pick.sudo ?: item.sudo
            Row(
                Modifier.fillMaxWidth().clickable { onPick(pick.copy(sudo = !sudo)) },
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(tr("Run with sudo"), modifier = Modifier.weight(1f))
                Switch(checked = sudo, onCheckedChange = null)
            }
            if (item.sudo) Hint(tr("it needs root"))

            item.password?.let { variable ->
                HorizontalDivider(color = Dusk.line)
                Heading(tr("Password"))
                val env = pick.env ?: model.core?.libraryEnv(item.index) ?: ""
                val options = listOf("" to tr("None: it asks, or goes without")) + envs.map { e ->
                    e.id to (if (e.names.contains(variable)) "${e.name} · ${tr("has {var}", "var" to variable)}" else e.name)
                }
                PickChip(
                    options.firstOrNull { it.first == env }?.second ?: options[0].second, env.isNotEmpty(),
                    options, { it.second },
                ) { onPick(pick.copy(env = it.first)) }
                Hint(if (envs.isEmpty()) tr("None yet: add one in Library › Envs.") else tr("It reads {var}: give it an environment that has it.", "var" to variable))
            }

            problem?.let { Text(it, color = Dusk.bad) }
            HorizontalDivider(color = Dusk.line)
            Row(Modifier.fillMaxWidth().clickable { onCheck(!checked) }, verticalAlignment = Alignment.CenterVertically) {
                Text(tr("Picked"), modifier = Modifier.weight(1f))
                Switch(checked = checked, onCheckedChange = null)
            }
            Button(onClick = {
                try {
                    model.core?.addFromLibrary(listOf(pick), under)
                    onCheck(false)
                    onAdded()
                } catch (e: Exception) {
                    problem = e.said()
                }
            }, modifier = Modifier.fillMaxWidth()) { Text(tr("Add to my snippets")) }
        }
    }
}
