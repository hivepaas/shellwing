package com.hivepaas.shellwing.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Checkbox
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import com.hivepaas.shellwing.AppModel
import com.hivepaas.shellwing.SessionModel
import com.hivepaas.shellwing.confirmOwner
import com.hivepaas.shellwing.core.EnvRow
import com.hivepaas.shellwing.core.SnippetEdit
import com.hivepaas.shellwing.core.SnippetRow
import com.hivepaas.shellwing.said
import com.hivepaas.shellwing.tr
import kotlinx.coroutines.delay

/** An editor's frame: Cancel, its title, Save; its fields scrolling under. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun EditorFrame(
    title: String,
    canSave: Boolean,
    onCancel: () -> Unit,
    onSave: () -> Unit,
    content: @Composable () -> Unit,
) {
    Scaffold(
        topBar = {
            TopAppBar(
                navigationIcon = { IconButton(onClick = onCancel) { Icon(Icons.Filled.Close, tr("Cancel")) } },
                title = { Text(title, maxLines = 1) },
                actions = { TextButton(onClick = onSave, enabled = canSave) { Text(tr("Save")) } },
            )
        },
    ) { pad ->
        Column(
            Modifier.fillMaxSize().padding(pad).imePadding().verticalScroll(rememberScrollState())
                .padding(horizontal = 16.dp, vertical = 8.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) { content() }
    }
}

/** A field for what is typed as is: no capitals, no corrections. */
@Composable
fun PlainField(
    label: String,
    value: String,
    mono: Boolean = false,
    lines: Int = 1,
    modifier: Modifier = Modifier.fillMaxWidth(),
    type: KeyboardType = KeyboardType.Text,
    placeholder: String? = null,
    onChange: (String) -> Unit,
) {
    OutlinedTextField(
        value = value, onValueChange = onChange, label = { Text(label) },
        placeholder = placeholder?.let { { Text(it, color = Dusk.muted) } },
        singleLine = lines == 1, minLines = lines,
        textStyle = if (mono) TextStyle(fontFamily = FontFamily.Monospace, fontSize = 14.sp) else TextStyle.Default,
        keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.None, autoCorrectEnabled = false, keyboardType = type),
        modifier = modifier,
    )
}

@Composable
fun Hint(text: String) = Text(text, fontSize = 13.sp, color = Dusk.muted)

// Snippets

@Composable
fun SnippetEditorScreen(model: AppModel, original: SnippetEdit, onDone: () -> Unit) {
    var edit by remember { mutableStateOf(original) }
    EditorFrame(
        title = if (original.id.isEmpty()) tr("New snippet") else original.name,
        canSave = edit.name.isNotBlank() && edit.command.isNotBlank(),
        onCancel = onDone,
        onSave = {
            try {
                model.core?.saveSnippet(edit)
                onDone()
            } catch (e: Exception) {
                model.error = e.said()
            }
        },
    ) {
        PlainField(tr("Name"), edit.name) { edit = edit.copy(name = it) }
        PlainField(tr("Category"), edit.category) { edit = edit.copy(category = it) }
        Heading(tr("Command"))
        PlainField(tr("Command"), edit.command, mono = true, lines = 5) { edit = edit.copy(command = it) }
        Hint(tr("{{name}} asks for a value when it runs; {{host}}, {{user}} and {{address}} come from the session."))
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(tr("Press Enter after it"), modifier = Modifier.weight(1f))
            Switch(checked = edit.enter, onCheckedChange = { edit = edit.copy(enter = it) })
        }
        ScopePicker(model, edit.scope) { edit = edit.copy(scope = it) }
        val envs = remember { model.core?.environments() ?: emptyList() }
        if (envs.isNotEmpty()) {
            Heading(tr("Runs with"))
            for (e in envs) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Checkbox(
                        checked = e.id in edit.needs,
                        onCheckedChange = { on ->
                            edit = edit.copy(needs = edit.needs.filter { it != e.id } + if (on) listOf(e.id) else emptyList())
                        },
                    )
                    Column {
                        Text(e.name)
                        Text(e.names.joinToString(" "), fontSize = 11.sp, color = Dusk.muted, fontFamily = FontFamily.Monospace, maxLines = 1)
                    }
                }
            }
            Hint(tr("Their variables are given to it alone, in a subshell, without showing their values."))
        }
    }
}

/** Runs a snippet in a session: its values asked first, when it has holes. */
@Composable
fun RunSnippet(model: AppModel, snippet: SnippetRow, session: SessionModel, onDone: (String?) -> Unit) {
    val run: (Map<String, String>) -> String? = { values ->
        val handle = session.handle
        val core = model.core
        if (core == null || handle == null) {
            tr("Not connected.")
        } else {
            try {
                core.runSnippet(handle, snippet.id, values)
                null
            } catch (e: Exception) {
                e.said()
            }
        }
    }
    if (snippet.asked.isEmpty()) {
        LaunchedEffect(snippet.id) { onDone(run(emptyMap())) }
        return
    }
    val values = remember { mutableStateMapOf<String, String>() }
    var problem by remember { mutableStateOf<String?>(null) }
    AlertDialog(
        onDismissRequest = { onDone(null) },
        title = { Text(snippet.name) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(snippet.command, fontFamily = FontFamily.Monospace, fontSize = 13.sp, color = Dusk.soft)
                Heading(tr("Values"))
                for (name in snippet.asked) {
                    PlainField(name, values[name] ?: "", mono = true) { values[name] = it }
                }
                problem?.let { Text(it, color = Dusk.bad) }
            }
        },
        confirmButton = {
            TextButton(onClick = {
                val why = run(values.toMap())
                if (why == null) onDone(null) else problem = why
            }) { Text(tr("Run")) }
        },
        dismissButton = { TextButton(onClick = { onDone(null) }) { Text(tr("Cancel")) } },
    )
}

// Environments

/**
 * An environment as `.env` text. Its variables are listed without values: a
 * name left so keeps the one it has; a value typed replaces it. Show values
 * puts them in, once the owner says it is them, for a minute, and hides them
 * when the app leaves the screen. As the iOS app's EnvEditor.
 */
@Composable
fun EnvEditorScreen(model: AppModel, env: EnvRow, onDone: () -> Unit) {
    val context = LocalContext.current
    var name by remember { mutableStateOf(env.name) }
    var scope by remember { mutableStateOf(env.scope) }
    var text by remember { mutableStateOf(env.names.joinToString("\n") { "$it=" }) }
    var shown by remember { mutableStateOf(false) }
    var shownAt by remember { mutableStateOf(0L) }
    var notShown by remember { mutableStateOf<List<String>>(emptyList()) }
    var problem by remember { mutableStateOf<String?>(null) }

    fun hide() {
        if (!shown) return
        text = model.core?.envTextHidden(env.id, text) ?: text
        shown = false
        notShown = emptyList()
    }

    fun show() {
        problem = null
        try {
            val r = model.core?.envTextShown(env.id, text) ?: return
            text = r.text
            notShown = r.notShown
            shown = true
            shownAt = System.currentTimeMillis()
        } catch (e: Exception) {
            problem = e.said()
        }
    }

    // A minute shown, then hidden again; out of sight, hidden at once.
    LaunchedEffect(shown, shownAt) {
        if (shown) {
            delay(60_000)
            hide()
        }
    }
    val owner = LocalLifecycleOwner.current
    DisposableEffect(owner) {
        val watch = LifecycleEventObserver { _, event -> if (event == Lifecycle.Event.ON_STOP) hide() }
        owner.lifecycle.addObserver(watch)
        onDispose { owner.lifecycle.removeObserver(watch) }
    }

    EditorFrame(
        title = if (env.id.isEmpty()) tr("New environment") else env.name,
        canSave = name.isNotBlank(),
        onCancel = onDone,
        onSave = {
            // What was shown and not changed is kept, not saved again.
            val dotenv = if (shown) model.core?.envTextHidden(env.id, text) ?: text else text
            try {
                model.core?.saveEnvironment(env.id, name, scope, dotenv)
                onDone()
            } catch (e: Exception) {
                model.error = e.said()
            }
        },
    ) {
        PlainField(tr("Name"), name) { name = it }
        ScopePicker(model, scope) { scope = it }
        Row(verticalAlignment = Alignment.CenterVertically) {
            Heading(tr("Variables"), Modifier.weight(1f))
            if (env.id.isNotEmpty()) {
                TextButton(onClick = {
                    if (shown) {
                        hide()
                    } else {
                        confirmOwner(context, tr("Show the values of {name}", "name" to env.name)) { ok -> if (ok) show() }
                    }
                }) { Text(if (shown) tr("Hide values") else tr("Show values")) }
            }
        }
        PlainField(tr("Variables"), text, mono = true, lines = 8, modifier = Modifier.fillMaxWidth().heightIn(min = 180.dp)) {
            text = it
        }
        Hint(tr("NAME=value, a line. A name with nothing after = keeps the value it has."))
        if (env.id.isNotEmpty()) Hint(tr("Values shown hide again after a minute, or when you leave the app."))
        if (notShown.isNotEmpty()) {
            Hint(tr("Not shown, of several lines: {names}. The desktop shows them.", "names" to notShown.joinToString(", ")))
        }
        problem?.let { Text(it, color = Dusk.bad) }
        Spacer(Modifier.width(1.dp))
    }
}
