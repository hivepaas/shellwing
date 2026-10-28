package com.hivepaas.shellwing.ui

import android.content.Intent
import androidx.compose.foundation.clickable
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
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.hivepaas.shellwing.AppModel
import com.hivepaas.shellwing.SessionModel
import com.hivepaas.shellwing.confirmOwner
import com.hivepaas.shellwing.core.NoteRow
import com.hivepaas.shellwing.core.toggleTask
import com.hivepaas.shellwing.said
import com.hivepaas.shellwing.tr

/** The notes, filtered; with [spec], those about its host first. */
@Composable
fun NotesList(model: AppModel, spec: String?, onPick: (NoteRow) -> Unit) {
    var notes by remember { mutableStateOf(model.core?.notes(spec) ?: emptyList()) }
    var filter by rememberSaveable { mutableStateOf("") }
    val q = filter.lowercase()
    val shown = notes.filter {
        q.isEmpty() || it.title.lowercase().contains(q) || it.category.lowercase().contains(q) ||
            it.hosts.any { h -> h.lowercase().contains(q) }
    }
    Library(
        filter = filter, onFilter = { filter = it }, filterHint = tr("Filter notes"),
        empty = if (notes.isEmpty()) tr("No notes yet.") else null,
    ) {
        for (n in shown) {
            item(key = n.id) {
                Item(
                    title = n.title.ifEmpty { tr("Untitled") },
                    sub = buildList {
                        add(n.category.ifEmpty { if (n.markdown) tr("Markdown") else tr("Text") })
                        if (n.hosts.isNotEmpty()) add(n.hosts.joinToString(", "))
                    }.joinToString(" · "),
                    onClick = { onPick(n) },
                    onDelete = if (spec == null) {
                        { model.core?.forget(n.id); notes = model.core?.notes(spec) ?: emptyList() }
                    } else null,
                    deleteTitle = if (n.title.isEmpty()) tr("Delete this note?") else tr("Delete {title}?", "title" to n.title),
                    trailing = {
                        if (n.here) Text(tr("this host"), fontSize = 11.sp, color = Dusk.accent)
                        if (n.sensitive) Icon(Icons.Filled.Lock, null, tint = Dusk.amber, modifier = Modifier.size(16.dp))
                    },
                )
            }
        }
    }
}

/**
 * A note as the desktop's note window has it: written, or shown as
 * Markdown; for hosts; sensitive; exported. Saved when it is left.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun NoteEditorScreen(model: AppModel, note: NoteRow, onDone: () -> Unit) {
    val context = LocalContext.current
    var id by remember { mutableStateOf(note.id) }
    var title by remember { mutableStateOf(note.title) }
    var category by remember { mutableStateOf(note.category) }
    var markdown by remember { mutableStateOf(note.markdown) }
    var sensitive by remember { mutableStateOf(note.sensitive) }
    var hosts by remember { mutableStateOf(note.hosts) }
    // A sensitive note's text is not loaded until the owner says it is them.
    var shown by remember { mutableStateOf(!note.sensitive) }
    var text by remember {
        mutableStateOf(if (!note.sensitive && note.id.isNotEmpty()) runCatching { model.core?.noteText(note.id) }.getOrNull() ?: "" else "")
    }
    var preview by remember { mutableStateOf(note.markdown && note.id.isNotEmpty()) }
    var menu by remember { mutableStateOf(false) }
    var choosing by remember { mutableStateOf(false) }
    var deleting by remember { mutableStateOf(false) }
    var deleted by remember { mutableStateOf(false) }

    fun save() {
        if (deleted || (title.isEmpty() && text.isEmpty() && id.isEmpty())) return
        try {
            // A sensitive note not shown was not loaded: its text stays.
            id = model.core?.saveNote(id, title, category, markdown, hosts, sensitive, if (shown) text else null) ?: id
        } catch (e: Exception) {
            model.error = tr("Not saved: {error}", "error" to e.said())
        }
    }
    val saving by rememberUpdatedState(::save)
    DisposableEffect(Unit) { onDispose { saving() } }

    Scaffold(
        topBar = {
            TopAppBar(
                navigationIcon = { IconButton(onClick = onDone) { Icon(Icons.AutoMirrored.Filled.ArrowBack, tr("Library")) } },
                title = {},
                actions = {
                    if (markdown && shown) {
                        TextButton(onClick = { preview = !preview }) { Text(if (preview) tr("Edit") else tr("Preview")) }
                    }
                    IconButton(onClick = { menu = true }) { Icon(Icons.Filled.MoreVert, tr("Note")) }
                    DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                        DropdownMenuItem(text = { Text(tr("For hosts…")) }, onClick = { menu = false; choosing = true })
                        DropdownMenuItem(
                            text = { Text(tr("Sensitive")) },
                            trailingIcon = { if (sensitive) Icon(Icons.Filled.Check, null) },
                            onClick = { menu = false; sensitive = !sensitive },
                        )
                        DropdownMenuItem(
                            text = { Text(tr("Markdown")) },
                            trailingIcon = { if (markdown) Icon(Icons.Filled.Check, null) },
                            onClick = { menu = false; markdown = !markdown },
                        )
                        if (shown) {
                            DropdownMenuItem(text = { Text(tr("Export…")) }, onClick = {
                                menu = false
                                val out = if (title.isEmpty()) text else if (markdown) "# $title\n\n$text" else "$title\n\n$text"
                                context.startActivity(
                                    Intent.createChooser(
                                        Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, out)
                                            .putExtra(Intent.EXTRA_SUBJECT, title.ifEmpty { tr("Note") }),
                                        null,
                                    ),
                                )
                            })
                        }
                        if (id.isNotEmpty()) {
                            DropdownMenuItem(text = { Text(tr("Delete"), color = Dusk.bad) }, onClick = { menu = false; deleting = true })
                        }
                    }
                },
            )
        },
    ) { pad ->
        Column(Modifier.fillMaxSize().padding(pad).imePadding()) {
            val bare = TextFieldDefaults.colors(
                focusedContainerColor = Color.Transparent, unfocusedContainerColor = Color.Transparent,
                focusedIndicatorColor = Color.Transparent, unfocusedIndicatorColor = Color.Transparent,
            )
            TextField(
                value = title, onValueChange = { title = it }, placeholder = { Text(tr("Title")) }, singleLine = true,
                textStyle = TextStyle(fontSize = 20.sp, fontWeight = FontWeight.SemiBold), colors = bare,
                modifier = Modifier.fillMaxWidth(),
            )
            Row(verticalAlignment = Alignment.CenterVertically) {
                TextField(
                    value = category, onValueChange = { category = it }, placeholder = { Text(tr("No category")) },
                    singleLine = true, textStyle = TextStyle(fontSize = 13.sp, color = Dusk.muted), colors = bare,
                    modifier = Modifier.weight(1f),
                )
                if (hosts.isNotEmpty()) {
                    Text(hosts.joinToString(", "), fontSize = 12.sp, color = Dusk.accent, maxLines = 1, modifier = Modifier.padding(end = 16.dp))
                }
            }
            HorizontalDivider(color = Dusk.line)
            when {
                !shown -> Column(
                    Modifier.fillMaxWidth().padding(top = 40.dp),
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    Icon(Icons.Filled.Lock, null, tint = Dusk.amber, modifier = Modifier.size(28.dp))
                    Text(tr("Sensitive: its text shows once it is you."), color = Dusk.muted)
                    OutlinedButton(onClick = {
                        confirmOwner(context, tr("Show a sensitive note")) { ok ->
                            if (ok) {
                                text = runCatching { model.core?.noteText(note.id) }.getOrNull() ?: ""
                                shown = true
                            }
                        }
                    }) { Text(tr("Show the text")) }
                }
                preview && markdown -> Column(Modifier.verticalScroll(rememberScrollState()).padding(16.dp)) {
                    MarkdownView(model, text, toggle = { at -> toggleTask(text, at)?.let { text = it } })
                }
                else -> TextField(
                    value = text, onValueChange = { text = it }, colors = bare,
                    textStyle = TextStyle(fontFamily = FontFamily.Monospace, fontSize = 15.sp),
                    // Notes hold commands: nothing typed is capitalized for you.
                    keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.None, autoCorrectEnabled = false),
                    modifier = Modifier.fillMaxSize(),
                )
            }
        }
    }

    if (choosing) HostsChooser(model, hosts, onChange = { hosts = it }, onDone = { choosing = false })
    if (deleting) {
        AlertDialog(
            onDismissRequest = { deleting = false },
            title = { Text(if (title.isEmpty()) tr("Delete this note?") else tr("Delete {title}?", "title" to title)) },
            text = { Text(goesEverywhere()) },
            confirmButton = {
                TextButton(onClick = {
                    model.core?.forget(id)
                    deleted = true
                    deleting = false
                    onDone()
                }) { Text(tr("Delete"), color = Dusk.bad) }
            },
            dismissButton = { TextButton(onClick = { deleting = false }) { Text(tr("Cancel")) } },
        )
    }
}

/** The hosts a note is about. */
@Composable
private fun HostsChooser(model: AppModel, chosen: List<String>, onChange: (List<String>) -> Unit, onDone: () -> Unit) {
    val names = (model.sections.flatMap { it.hosts }.map { it.name } + chosen).toSortedSet().toList()
    AlertDialog(
        onDismissRequest = onDone,
        title = { Text(tr("For hosts")) },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState())) {
                for (name in names) {
                    Row(
                        Modifier.fillMaxWidth().clickable {
                            onChange(if (name in chosen) chosen - name else chosen + name)
                        }.padding(vertical = 12.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text(name, modifier = Modifier.weight(1f))
                        if (name in chosen) Icon(Icons.Filled.Check, null, tint = Dusk.accent)
                    }
                }
            }
        },
        confirmButton = { TextButton(onClick = onDone) { Text(tr("Done")) } },
    )
}

/** A note read in a session: its commands inserted into the shell; a task ticked is saved at once. */
@Composable
fun NoteReader(model: AppModel, note: NoteRow, session: SessionModel, onDone: () -> Unit) {
    val context = LocalContext.current
    var text by remember {
        mutableStateOf(if (note.sensitive) null else runCatching { model.core?.noteText(note.id) }.getOrNull())
    }
    Column(Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).padding(16.dp)) {
        Text(note.title.ifEmpty { tr("Untitled") }, fontSize = 22.sp, fontWeight = FontWeight.SemiBold)
        Spacer(Modifier.height(12.dp))
        val now = text
        when {
            now == null -> OutlinedButton(onClick = {
                confirmOwner(context, tr("Show a sensitive note")) { ok ->
                    if (ok) text = runCatching { model.core?.noteText(note.id) }.getOrNull()
                }
            }) { Text(tr("Show the text")) }
            note.markdown -> MarkdownView(
                model, now,
                insert = { code ->
                    session.paste(code)
                    onDone()
                },
                toggle = { at ->
                    toggleTask(now, at)?.let { ticked ->
                        text = ticked
                        try {
                            model.core?.saveNote(note.id, note.title, note.category, note.markdown, note.hosts, note.sensitive, ticked)
                        } catch (e: Exception) {
                            model.error = e.said()
                        }
                    }
                },
            )
            else -> Text(now, fontFamily = FontFamily.Monospace, fontSize = 14.sp)
        }
        Spacer(Modifier.width(1.dp).height(32.dp))
    }
}
