package com.hivepaas.shellwing.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Group
import androidx.compose.material.icons.outlined.Key
import androidx.compose.material.icons.outlined.Person
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.hivepaas.shellwing.AppModel
import com.hivepaas.shellwing.core.AccessGrant
import com.hivepaas.shellwing.core.CollectionAccess
import com.hivepaas.shellwing.core.WaitingMember
import com.hivepaas.shellwing.said
import com.hivepaas.shellwing.tr
import kotlinx.coroutines.launch

/**
 * The secrets of a collection whose key this phone replaced, which somebody
 * who lost access could read: to change where they are used, then mark done.
 */
@Composable
fun KeyChecklistScreen(model: AppModel, collection: String, onBack: () -> Unit) {
    val list = model.checklists.firstOrNull { it.collection == collection }
    // Done meanwhile, on this screen or another: nothing left to show.
    LaunchedEffect(list == null) { if (list == null) onBack() }
    SubScreen(list?.name ?: "", onBack) {
        if (list == null) return@SubScreen
        LazyColumn(Modifier.fillMaxSize()) {
            item {
                Text(
                    tr("Somebody who lost access to {name} could read these: change them", "name" to list.name),
                    fontSize = 14.sp, color = Dusk.muted,
                    modifier = Modifier.padding(16.dp),
                )
            }
            items(list.secrets) { name ->
                Row(
                    Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Icon(Icons.Outlined.Key, null, tint = Dusk.soft, modifier = Modifier.size(20.dp))
                    Spacer(Modifier.width(16.dp))
                    Text(name, fontSize = 16.sp)
                }
            }
            item {
                Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    Text(tr("Change each where it is used, then here in the collection."), fontSize = 14.sp, color = Dusk.muted)
                    Button(onClick = { model.finishChecklist(list.collection); onBack() }) {
                        Text(tr("All changed: take this list away"))
                    }
                }
            }
        }
    }
}

/**
 * Who waits for a collection's key, for somebody who manages it and holds
 * it: each with the fingerprint to compare with their Profile page before
 * giving it.
 */
@Composable
fun GiveKeyScreen(model: AppModel, collection: String, onBack: () -> Unit) {
    val name = model.collections.firstOrNull { it.id == collection }?.name ?: ""
    var members by remember { mutableStateOf<List<WaitingMember>?>(null) }
    var problem by remember { mutableStateOf<String?>(null) }
    var given by remember { mutableStateOf<String?>(null) }
    var asking by remember { mutableStateOf<WaitingMember?>(null) }
    var giving by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    suspend fun load() {
        try {
            members = model.core?.waitingForKey(collection)
            problem = null
        } catch (e: Exception) {
            problem = tr("Cannot list who waits: {e}", "e" to e.said())
        }
    }
    LaunchedEffect(collection) { load() }

    SubScreen(tr("Give the key of {name} to whom?", "name" to name), onBack) {
        LazyColumn(Modifier.fillMaxSize()) {
            problem?.let { item { Text(it, color = Dusk.bad, modifier = Modifier.padding(16.dp)) } }
            given?.let { item { Text(it, color = Dusk.ok, modifier = Modifier.padding(16.dp)) } }
            if (members?.isEmpty() == true) {
                item { Text(tr("Everybody with access holds the key."), color = Dusk.muted, modifier = Modifier.padding(16.dp)) }
            }
            items(members ?: emptyList(), key = { it.userId }) { m ->
                Column(Modifier.fillMaxWidth().padding(16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    Text(m.name, fontSize = 16.sp, fontWeight = FontWeight.Medium)
                    val f = m.fingerprint
                    when {
                        f != null && m.served -> {
                            Text(f, fontSize = 15.sp, fontFamily = FontFamily.Monospace)
                            Text(
                                tr("Fingerprint {fingerprint}: compare it with the one on their Profile page, then give.", "fingerprint" to f),
                                fontSize = 13.sp, color = Dusk.muted,
                            )
                            OutlinedButton(onClick = { asking = m }, enabled = !giving) { Text(tr("Give")) }
                        }
                        f != null -> Text(tr("Suspended in the organization: they can have it once they are back."), fontSize = 13.sp, color = Dusk.muted)
                        else -> Text(tr("Has no encryption key yet: they sync the shellwing app with the server once."), fontSize = 13.sp, color = Dusk.muted)
                    }
                }
            }
        }
    }

    asking?.let { m ->
        AlertDialog(
            onDismissRequest = { asking = null },
            title = { Text(tr("Give the key to {name}?", "name" to m.name)) },
            text = { Text(tr("Fingerprint {fingerprint}: compare it with the one on their Profile page, then give.", "fingerprint" to (m.fingerprint ?: ""))) },
            confirmButton = {
                TextButton(onClick = {
                    asking = null
                    giving = true
                    scope.launch {
                        try {
                            model.core?.giveKey(collection, m.userId)
                            given = tr("{name} has the key of {collection} now.", "name" to m.name, "collection" to name)
                            problem = null
                            load()
                        } catch (e: Exception) {
                            problem = tr("The key was not given: {e}", "e" to e.said())
                        }
                        giving = false
                    }
                }) { Text(tr("Give")) }
            },
            dismissButton = { TextButton(onClick = { asking = null }) { Text(tr("Cancel")) } },
        )
    }
}

/** A permission's name, and what it lets people do. */
private fun permissionText(p: String): Pair<String, String> = when (p) {
    "connect" -> tr("Connect") to tr("Connects to its hosts; cannot change them")
    "edit" -> tr("Edit") to tr("Changes its hosts and passwords")
    else -> tr("Manage") to tr("Edits, grants access and gives the key")
}

private val PERMISSIONS = listOf("connect", "edit", "manage")

/**
 * Who has access to a collection, for somebody who manages it: each grant's
 * permission changed or taken away, people and groups granted it, then saved
 * at once, as the web app does.
 */
@Composable
fun AccessScreen(model: AppModel, collection: String, onBack: () -> Unit) {
    val name = model.collections.firstOrNull { it.id == collection }?.name ?: ""
    var access by remember { mutableStateOf<CollectionAccess?>(null) }
    var grants by remember { mutableStateOf<List<AccessGrant>>(emptyList()) }
    var problem by remember { mutableStateOf<String?>(null) }
    var saved by remember { mutableStateOf<String?>(null) }
    var saving by remember { mutableStateOf(false) }
    var adding by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    suspend fun load() {
        try {
            val a = model.core?.collectionAccess(collection) ?: return
            access = a
            grants = a.grants
            problem = null
        } catch (e: Exception) {
            problem = tr("Cannot read who has access: {e}", "e" to e.said())
        }
    }
    LaunchedEffect(collection) { load() }
    val changed = access != null && access?.grants != grants
    fun same(a: AccessGrant, b: AccessGrant) = a.userId == b.userId && a.groupId == b.groupId

    SubScreen(tr("Who has access to {name}", "name" to name), onBack) {
        LazyColumn(Modifier.fillMaxSize()) {
            problem?.let { item { Text(it, color = Dusk.bad, modifier = Modifier.padding(16.dp)) } }
            saved?.let { item { Text(it, color = Dusk.ok, modifier = Modifier.padding(16.dp)) } }
            items(grants, key = { it.userId + "|" + it.groupId }) { g ->
                var menu by remember { mutableStateOf(false) }
                Row(
                    Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 10.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Icon(if (g.groupId.isEmpty()) Icons.Outlined.Person else Icons.Outlined.Group, null, tint = Dusk.muted)
                    Spacer(Modifier.width(16.dp))
                    Text(g.name, fontSize = 16.sp, modifier = Modifier.weight(1f))
                    Box {
                        TextButton(onClick = { menu = true }) { Text(permissionText(g.permission).first) }
                        DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                            for (p in PERMISSIONS) {
                                DropdownMenuItem(
                                    text = {
                                        Column {
                                            Text(permissionText(p).first)
                                            Text(permissionText(p).second, fontSize = 12.sp, color = Dusk.muted)
                                        }
                                    },
                                    onClick = {
                                        menu = false
                                        grants = grants.map { if (same(it, g)) it.copy(permission = p) else it }
                                    },
                                )
                            }
                            DropdownMenuItem(
                                text = {
                                    Column {
                                        Text(tr("Take access away"), color = Dusk.bad)
                                        Text(tr("The collection gets a new key, which they never have"), fontSize = 12.sp, color = Dusk.muted)
                                    }
                                },
                                onClick = { menu = false; grants = grants.filterNot { same(it, g) } },
                            )
                        }
                    }
                }
            }
            access?.let { a ->
                item {
                    Box(Modifier.padding(horizontal = 16.dp, vertical = 8.dp)) {
                        OutlinedButton(onClick = { adding = true }) { Text(tr("Grant somebody…")) }
                        DropdownMenu(expanded = adding, onDismissRequest = { adding = false }) {
                            val people = a.people.filter { p -> grants.none { it.userId == p.userId } }
                            val groups = a.groups.filter { g -> grants.none { it.groupId == g.groupId } }
                            if (people.isEmpty() && groups.isEmpty()) {
                                DropdownMenuItem(text = { Text(tr("Everybody of the organization has access already.")) }, onClick = { adding = false })
                            }
                            for (p in people) {
                                DropdownMenuItem(
                                    text = {
                                        Column {
                                            Text(p.name)
                                            val why = when {
                                                p.owner -> tr("Owner: has Manage already")
                                                p.suspended -> tr("Suspended: has it once active again")
                                                else -> null
                                            }
                                            why?.let { Text(it, fontSize = 12.sp, color = Dusk.muted) }
                                        }
                                    },
                                    onClick = {
                                        adding = false
                                        grants = grants + AccessGrant(p.userId, "", p.name, "connect")
                                    },
                                )
                            }
                            for (g in groups) {
                                DropdownMenuItem(
                                    text = {
                                        Column {
                                            Text(g.name)
                                            Text(tr("A group: everybody in it"), fontSize = 12.sp, color = Dusk.muted)
                                        }
                                    },
                                    onClick = {
                                        adding = false
                                        grants = grants + AccessGrant("", g.groupId, g.name, "connect")
                                    },
                                )
                            }
                        }
                    }
                }
                item {
                    Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                        for (p in PERMISSIONS) {
                            Text("${permissionText(p).first}: ${permissionText(p).second}", fontSize = 13.sp, color = Dusk.muted)
                        }
                    }
                }
                item {
                    Button(
                        onClick = {
                            val before = a.grants
                            saving = true
                            scope.launch {
                                try {
                                    model.core?.setAccess(collection, grants)
                                    val added = grants.any { g -> before.none { same(it, g) } }
                                    saved = if (added) {
                                        tr("Access saved. Anybody newly granted waits for the key: give it from the key button.")
                                    } else {
                                        tr("Access saved.")
                                    }
                                    problem = null
                                    load()
                                } catch (e: Exception) {
                                    problem = tr("Access was not changed: {e}", "e" to e.said())
                                }
                                saving = false
                            }
                        },
                        enabled = changed && !saving,
                        modifier = Modifier.padding(horizontal = 16.dp),
                    ) { Text(tr("Save")) }
                }
            }
        }
    }
}
