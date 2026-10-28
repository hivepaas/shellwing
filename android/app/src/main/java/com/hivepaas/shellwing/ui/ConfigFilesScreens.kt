package com.hivepaas.shellwing.ui

import android.provider.OpenableColumns
import android.text.format.Formatter
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.hivepaas.shellwing.AppModel
import com.hivepaas.shellwing.SessionModel
import com.hivepaas.shellwing.core.ConfigFileRow
import com.hivepaas.shellwing.core.Found
import com.hivepaas.shellwing.core.Looked
import com.hivepaas.shellwing.core.PlaceHow
import com.hivepaas.shellwing.core.SessionStatus
import com.hivepaas.shellwing.said
import com.hivepaas.shellwing.tr
import kotlinx.coroutines.launch

/** Config files by category: kept in the vault, placed on a session's host. */
@Composable
fun ConfigFilesList(model: AppModel, onPick: (ConfigFileRow) -> Unit) {
    var files by remember { mutableStateOf(model.core?.configFiles(null) ?: emptyList()) }
    Library(
        empty = if (files.isEmpty()) {
            tr("No config files yet: files kept in the vault (a kubeconfig, an .npmrc), placed on a host from a session.")
        } else null,
    ) {
        for ((cat, group) in files.groupBy { it.category }) {
            item(key = "cat:$cat") { Heading(cat.ifEmpty { tr("No category") }, Modifier.padding(start = 16.dp, top = 12.dp)) }
            items(group, key = { it.id }) { f ->
                Item(
                    f.name, f.path, mono = true,
                    onClick = { onPick(f) },
                    onDelete = { model.core?.forget(f.id); files = model.core?.configFiles(null) ?: emptyList() },
                    deleteTitle = tr("Delete {name}?", "name" to f.name),
                    trailing = { if (f.stays) Text(tr("stays"), fontSize = 11.sp, color = Dusk.muted) },
                )
            }
        }
    }
}

@Composable
fun ConfigFileEditorScreen(model: AppModel, original: ConfigFileRow, onDone: () -> Unit) {
    val context = LocalContext.current
    var file by remember { mutableStateOf(original) }
    var mode by remember { mutableStateOf(original.mode.toString(8)) }
    var changed by remember { mutableStateOf(false) }
    var text by remember {
        mutableStateOf(
            if (original.id.isNotEmpty() && !original.binary) runCatching { model.core?.configFileText(original.id) }.getOrNull() ?: "" else "",
        )
    }
    var problem by remember { mutableStateOf<String?>(null) }
    val importer = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        uri ?: return@rememberLauncherForActivityResult
        val read = runCatching {
            context.contentResolver.openInputStream(uri)?.use { it.readBytes() }?.toString(Charsets.UTF_8)
        }.getOrNull()
        if (read == null || read.contains('\u0000')) {
            problem = tr("Only a text file can be imported here.")
            return@rememberLauncherForActivityResult
        }
        text = read
        changed = true
        if (file.name.isEmpty()) {
            context.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
                if (c.moveToFirst()) file = file.copy(name = c.getString(0))
            }
        }
    }

    EditorFrame(
        title = if (original.id.isEmpty()) tr("New config file") else original.name,
        canSave = file.name.isNotBlank() && file.path.isNotBlank(),
        onCancel = onDone,
        onSave = {
            val m = mode.toUIntOrNull(8)
            if (m == null || m > "777".toUInt(8)) {
                problem = tr("Permissions: three digits from 0 to 7, such as 600.")
            } else {
                try {
                    model.core?.saveConfigFile(file.copy(mode = m), if (changed || file.id.isEmpty()) text else null)
                    onDone()
                } catch (e: Exception) {
                    problem = e.said()
                }
            }
        },
    ) {
        PlainField(tr("Name"), file.name) { file = file.copy(name = it) }
        PlainField(tr("Category"), file.category) { file = file.copy(category = it) }
        Heading(tr("On the host"))
        PlainField("~/.kube/config", file.path, mono = true) { file = file.copy(path = it) }
        PlainField(tr("Permissions"), mode, mono = true) { mode = it }
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(tr("Placed to stay"), modifier = Modifier.weight(1f))
            Switch(checked = file.stays, onCheckedChange = { file = file.copy(stays = it) })
        }
        Hint(
            if (file.stays) tr("Placed, it stays when the session ends.")
            else tr("Placed, it is taken away when the session ends."),
        )
        ScopePicker(model, file.scope) { file = file.copy(scope = it) }
        Heading(tr("Content"))
        if (file.binary && !changed) {
            Hint(tr("Binary, {size}: kept as it is.", "size" to Formatter.formatShortFileSize(context, file.size.toLong())))
        } else {
            PlainField(tr("Content"), text, mono = true, lines = 8, modifier = Modifier.fillMaxWidth().heightIn(min = 200.dp)) {
                text = it
                changed = true
            }
        }
        OutlinedButton(onClick = { importer.launch(arrayOf("*/*")) }) { Text(tr("Replace from Files…")) }
        problem?.let { Text(it, color = Dusk.bad) }
    }
}

/**
 * Places a config file on the session's host: looked at first, then the
 * choices that fit what is there (nothing, the same, another file, one
 * placed by this app). As the iOS app's PlaceConfigSheet.
 */
@Composable
fun PlaceConfig(model: AppModel, session: SessionModel, onDone: (String?) -> Unit) {
    val scope = rememberCoroutineScope()
    val files = remember { model.core?.configFiles(session.spec) ?: emptyList() }
    var asking by remember { mutableStateOf<Pair<ConfigFileRow, Looked>?>(null) }
    var busy by remember { mutableStateOf(false) }
    var problem by remember { mutableStateOf<String?>(null) }

    fun look(f: ConfigFileRow) {
        val core = model.core
        val handle = session.handle
        if (core == null || handle == null || session.status != SessionStatus.CONNECTED) {
            problem = tr("Not connected.")
            return
        }
        busy = true
        problem = null
        scope.launch {
            try {
                val looked = core.lookToPlace(handle, f.id, f.path)
                if (looked.found == Found.SAME && !looked.ours) {
                    onDone(tr("{path} holds it already.", "path" to looked.full))
                } else {
                    asking = f to looked
                }
            } catch (e: Exception) {
                problem = tr("Cannot place {name}: {error}", "name" to f.name, "error" to e.said())
            }
            busy = false
        }
    }

    fun place(f: ConfigFileRow, looked: Looked, how: PlaceHow?) {
        val core = model.core ?: return
        val handle = session.handle ?: return
        busy = true
        asking = null
        scope.launch {
            try {
                if (how != null) core.place(handle, f.id, looked.full, how) else core.keepPlaced(handle, looked.full)
                session.changed()
                val until = how == PlaceHow.UNTIL || how == PlaceHow.UNTIL_OVER
                onDone(
                    if (until) tr("{name} is at {path} until the session ends.", "name" to f.name, "path" to looked.full)
                    else tr("{name} is at {path}.", "name" to f.name, "path" to looked.full),
                )
            } catch (e: Exception) {
                problem = tr("Cannot place {name}: {error}", "name" to f.name, "error" to e.said())
            }
            busy = false
        }
    }

    Column {
        Heading(tr("Place a config file"), Modifier.padding(16.dp))
        if (busy) LinearProgressIndicator(Modifier.fillMaxWidth())
        if (files.isEmpty()) Text(tr("No config files for this host. Keep them in Library › Files."), color = Dusk.muted, modifier = Modifier.padding(16.dp))
        problem?.let { Text(it, color = Dusk.bad, modifier = Modifier.padding(16.dp)) }
        LazyColumn {
            items(files, key = { it.id }) { f -> Item(f.name, f.path, mono = true, onClick = { if (!busy) look(f) }) }
        }
    }

    asking?.let { (f, looked) ->
        AlertDialog(
            onDismissRequest = { asking = null },
            title = {
                Text(
                    if (looked.found == Found.OTHER && !looked.ours) tr("{path} is there already…", "path" to looked.full)
                    else tr("Place at {path}…", "path" to looked.full),
                )
            },
            text = {
                Column {
                    for ((label, how) in placeChoices(f, looked)) {
                        TextButton(onClick = { place(f, looked, how) }, modifier = Modifier.fillMaxWidth()) { Text(label) }
                    }
                }
            },
            confirmButton = {},
            dismissButton = { TextButton(onClick = { asking = null }) { Text(tr("Cancel")) } },
        )
    }
}

/** What can be done with what is there; the file's own way (to stay, or until the session ends) first. */
private fun placeChoices(f: ConfigFileRow, looked: Looked): List<Pair<String, PlaceHow?>> {
    fun first(stay: List<Pair<String, PlaceHow?>>, until: List<Pair<String, PlaceHow?>>) =
        if (f.stays) stay + until else until + stay
    if (looked.ours) {
        val again = if (looked.found == Found.SAME) emptyList() else listOf(tr("Replace the one placed until the session ends") to PlaceHow.UNTIL)
        return first(listOf(tr("Keep the one placed here") to null), again)
    }
    return when (looked.found) {
        Found.NOTHING, Found.SAME -> first(
            listOf(tr("Place it to stay") to PlaceHow.STAY),
            listOf(tr("Place it until the session ends") to PlaceHow.UNTIL),
        )
        Found.OTHER -> first(
            listOf(tr("Replace it, keeping the old one") to PlaceHow.BACKUP, tr("Replace it") to PlaceHow.OVER),
            listOf(tr("Replace it until the session ends") to PlaceHow.UNTIL_OVER),
        )
    }
}
