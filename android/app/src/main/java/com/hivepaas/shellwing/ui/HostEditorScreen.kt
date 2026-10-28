package com.hivepaas.shellwing.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExposedDropdownMenuAnchorType
import androidx.compose.material3.ExposedDropdownMenuBox
import androidx.compose.material3.ExposedDropdownMenuDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Switch
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
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.hivepaas.shellwing.AppModel
import com.hivepaas.shellwing.SessionModel
import com.hivepaas.shellwing.core.AuthWay
import com.hivepaas.shellwing.core.HostEdit
import com.hivepaas.shellwing.said
import com.hivepaas.shellwing.tr

/** The shell kept on the host: tmux's session `shellwing`, as the iOS app sets it. */
private const val TMUX = "command -v tmux >/dev/null && tmux new -A -s shellwing"

private fun blank() = HostEdit(
    id = null, name = "", alias = null, address = "", port = null, user = null, group = null,
    tags = emptyList(), jump = emptyList(), forward = emptyList(), auth = AuthWay.AUTO, key = null, theme = null,
    color = null, favorite = false, localOnly = false, forwardAgent = false, keepalive = null, hostKey = null,
    run = null,
)

/**
 * A host's fields: where it is, who signs in and how, its group, its shell
 * kept in tmux. What the phone does not edit (jumps, forwards, a theme)
 * stays as hosts.toml has it.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun HostEditorScreen(model: AppModel, hostId: String?, onDone: () -> Unit, onConnect: (SessionModel) -> Unit) {
    var edit by remember { mutableStateOf(hostId?.let { model.host(it) } ?: blank()) }
    var port by remember { mutableStateOf(edit.port?.toString() ?: "") }
    var problem by remember { mutableStateOf<String?>(null) }
    var deleting by remember { mutableStateOf(false) }

    fun save(connect: Boolean) {
        val p = port.trim()
        val number = p.toIntOrNull()
        if (p.isNotEmpty() && (number == null || number !in 1..65535)) {
            problem = tr("Port: 1 to 65535.")
            return
        }
        val done = edit.copy(port = number?.toUShort())
        try {
            val id = model.save(done)
            val spec = model.sections.flatMap { it.hosts }.firstOrNull { it.id == id }?.spec
            if (connect && spec != null) model.session(spec)?.let { onConnect(it); return }
            onDone()
        } catch (e: Exception) {
            problem = e.said()
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                navigationIcon = { IconButton(onClick = onDone) { Icon(Icons.Filled.Close, tr("Cancel")) } },
                title = { Text(if (hostId == null) tr("New host") else edit.name) },
                actions = { TextButton(onClick = { save(false) }) { Text(tr("Save")) } },
            )
        },
    ) { pad ->
        Column(
            Modifier.fillMaxSize().padding(pad).imePadding().verticalScroll(rememberScrollState())
                .padding(horizontal = 16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Section(tr("Connection"))
            Field(tr("Name"), edit.name) { edit = edit.copy(name = it) }
            Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Field(tr("Address"), edit.address, Modifier.weight(2f)) { edit = edit.copy(address = it) }
                Field(tr("Port"), port, Modifier.weight(1f), KeyboardType.Number) { port = it }
            }
            Field(tr("User"), edit.user ?: "") { edit = edit.copy(user = it.ifBlank { null }) }

            Section(tr("Sign in"))
            val ways = listOf(AuthWay.AUTO to tr("Automatic"), AuthWay.KEY to tr("Key"), AuthWay.PASSWORD to tr("Password"))
            SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
                ways.forEachIndexed { i, (way, label) ->
                    SegmentedButton(
                        selected = edit.auth == way,
                        onClick = { edit = edit.copy(auth = way) },
                        shape = SegmentedButtonDefaults.itemShape(i, ways.size),
                    ) { Text(label) }
                }
            }
            Text(
                when (edit.auth) {
                    AuthWay.PASSWORD -> tr("Asked for when connecting, and can be saved in the vault.")
                    else -> tr("Tries the keys in the vault, then asks for a password, like ssh.")
                },
                fontSize = 13.sp, color = Dusk.muted,
            )

            Section(tr("Organize"))
            GroupPicker(model.groups(), edit.group) { edit = edit.copy(group = it) }

            Section(tr("Terminal"))
            Row(verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text(tr("Keep the shell on the host (tmux)"), fontSize = 16.sp)
                    Text(
                        tr("In tmux, a connection made again finds the same shell, its programs still running: the phone may lose it in the background. tmux has to be on the host; without it, the shell is as usual."),
                        fontSize = 13.sp, color = Dusk.muted,
                    )
                }
                Spacer(Modifier.width(16.dp))
                Switch(checked = edit.run == TMUX, onCheckedChange = { edit = edit.copy(run = if (it) TMUX else null) })
            }

            problem?.let { Text(it, color = Dusk.bad) }

            Row(horizontalArrangement = Arrangement.spacedBy(12.dp), modifier = Modifier.padding(top = 8.dp)) {
                OutlinedButton(onClick = { save(false) }, modifier = Modifier.weight(1f)) { Text(tr("Save")) }
                Button(onClick = { save(true) }, modifier = Modifier.weight(1f)) { Text(tr("Save and connect")) }
            }
            if (hostId != null) {
                TextButton(onClick = { deleting = true }) { Text(tr("Delete…"), color = Dusk.bad) }
            }
            Spacer(Modifier.height(32.dp))
        }
    }

    if (deleting && hostId != null) {
        AlertDialog(
            onDismissRequest = { deleting = false },
            title = { Text(tr("Delete {title} from hosts.toml?", "title" to edit.name)) },
            confirmButton = {
                TextButton(onClick = { model.delete(hostId); deleting = false; onDone() }) {
                    Text(tr("Delete"), color = Dusk.bad)
                }
            },
            dismissButton = { TextButton(onClick = { deleting = false }) { Text(tr("Cancel")) } },
        )
    }
}

@Composable
private fun Section(title: String) {
    Text(title, color = Dusk.accent, fontSize = 14.sp, modifier = Modifier.padding(top = 8.dp))
}

@Composable
private fun Field(
    label: String,
    value: String,
    modifier: Modifier = Modifier.fillMaxWidth(),
    type: KeyboardType = KeyboardType.Text,
    onChange: (String) -> Unit,
) {
    OutlinedTextField(
        value = value, onValueChange = onChange, label = { Text(label) }, singleLine = true,
        keyboardOptions = KeyboardOptions(keyboardType = type, autoCorrectEnabled = false),
        modifier = modifier,
    )
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun GroupPicker(groups: List<String>, value: String?, onChange: (String?) -> Unit) {
    var open by remember { mutableStateOf(false) }
    ExposedDropdownMenuBox(expanded = open, onExpandedChange = { open = it }) {
        OutlinedTextField(
            value = value ?: tr("None"), onValueChange = {}, readOnly = true, label = { Text(tr("Group")) },
            trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(open) },
            modifier = Modifier.fillMaxWidth().menuAnchor(ExposedDropdownMenuAnchorType.PrimaryNotEditable),
        )
        ExposedDropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            DropdownMenuItem(text = { Text(tr("None")) }, onClick = { onChange(null); open = false })
            for (g in groups) DropdownMenuItem(text = { Text(g) }, onClick = { onChange(g); open = false })
        }
    }
}
