package com.hivepaas.shellwing.ui

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Star
import androidx.compose.material.icons.outlined.Dns
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.hivepaas.shellwing.AppModel
import com.hivepaas.shellwing.R
import com.hivepaas.shellwing.SessionModel
import com.hivepaas.shellwing.core.HostRow
import com.hivepaas.shellwing.core.SessionStatus
import com.hivepaas.shellwing.statusWord
import com.hivepaas.shellwing.tr
import com.hivepaas.shellwing.trN

/** The hosts by group, the sessions open, a filter; a tap opens a session. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun HostsScreen(model: AppModel, onOpen: (Screen) -> Unit, onSessions: () -> Unit) {
    var filter by rememberSaveable { mutableStateOf("") }
    var doomed by remember { mutableStateOf<HostRow?>(null) }
    val connect: (String) -> Unit = { spec -> model.session(spec)?.let { onOpen(Screen.Terminal(it.id)) } }
    val another: (String) -> Unit = { spec -> model.open(spec)?.let { onOpen(Screen.Terminal(it.id)) } }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("shellwing") },
                navigationIcon = {
                    Icon(
                        painterResource(R.drawable.ic_notification), null,
                        tint = Dusk.accent, modifier = Modifier.padding(start = 16.dp, end = 8.dp).size(26.dp),
                    )
                },
                actions = { SyncButton(model) { onOpen(Screen.Sync) } },
            )
        },
        floatingActionButton = {
            ExtendedFloatingActionButton(
                onClick = { onOpen(Screen.HostEditor(null)) },
                icon = { Icon(Icons.Filled.Add, null) },
                text = { Text(tr("New host")) },
                containerColor = Dusk.accent,
                contentColor = Dusk.onAccent,
            )
        },
    ) { pad ->
        val words = filter.trim().lowercase()
        val sections = model.sections.mapNotNull { s ->
            val hosts = s.hosts.filter { h ->
                words.isEmpty() || h.name.lowercase().contains(words) || h.detail.lowercase().contains(words) ||
                    h.tags.any { it.lowercase().contains(words) }
            }
            if (hosts.isEmpty()) null else s.copy(hosts = hosts)
        }
        LazyColumn(Modifier.fillMaxSize().padding(pad)) {
            item {
                TextField(
                    value = filter,
                    onValueChange = { filter = it },
                    placeholder = { Text(tr("Filter by name, tag or address")) },
                    leadingIcon = { Icon(Icons.Outlined.Search, null) },
                    singleLine = true,
                    shape = RoundedCornerShape(28.dp),
                    colors = TextFieldDefaults.colors(
                        focusedIndicatorColor = Color.Transparent,
                        unfocusedIndicatorColor = Color.Transparent,
                        focusedContainerColor = Dusk.chip,
                        unfocusedContainerColor = Dusk.chip,
                    ),
                    modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp),
                )
            }
            if (model.sessions.isNotEmpty()) {
                item {
                    Heading(tr("Open"), Modifier.padding(start = 16.dp, top = 12.dp, bottom = 8.dp))
                    LazyRow(
                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                        contentPadding = androidx.compose.foundation.layout.PaddingValues(horizontal = 16.dp),
                    ) {
                        items(model.sessions, key = { it.id }) { s -> OpenChip(s) { onOpen(Screen.Terminal(s.id)) } }
                    }
                }
            }
            if (model.problem != null) {
                item { Text(model.problem, color = Dusk.bad, modifier = Modifier.padding(16.dp)) }
            } else if (model.sections.isEmpty()) {
                item {
                    Column(Modifier.fillMaxWidth().padding(32.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                        Text(tr("No hosts yet"), style = MaterialTheme.typography.titleMedium)
                        Spacer(Modifier.height(8.dp))
                        Text(
                            tr("Add one with +, or sync the hosts of your other devices."),
                            color = Dusk.muted, textAlign = TextAlign.Center,
                        )
                    }
                }
            }
            for (section in sections) {
                item(key = "section:" + (if (section.favorites) "*" else "") + section.name) {
                    Row(
                        Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, top = 16.dp, bottom = 4.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        section.color?.let {
                            Box(Modifier.size(10.dp).clip(RoundedCornerShape(3.dp)).background(rgb(it)))
                            Spacer(Modifier.width(10.dp))
                        }
                        if (section.favorites) {
                            Icon(Icons.Filled.Star, null, tint = Dusk.amber, modifier = Modifier.size(14.dp))
                            Spacer(Modifier.width(8.dp))
                        }
                        Heading(section.name ?: tr("Hosts"), Modifier.weight(1f))
                        Text(
                            trN(section.hosts.size, "{n} host", "{n} hosts"),
                            fontSize = 12.sp, color = Dusk.muted,
                        )
                    }
                }
                items(section.hosts, key = { (if (section.favorites) "favorite:" else "") + it.id }) { host ->
                    HostItem(
                        host = host,
                        session = model.sessions.lastOrNull { it.spec == host.spec },
                        onConnect = { connect(host.spec) },
                        onAnother = { another(host.spec) },
                        onEdit = { onOpen(Screen.HostEditor(host.id)) },
                        onDelete = { doomed = host },
                    )
                }
            }
            item { Spacer(Modifier.height(96.dp)) }
        }
    }

    doomed?.let { host ->
        AlertDialog(
            onDismissRequest = { doomed = null },
            title = { Text(tr("Delete {title} from hosts.toml?", "title" to host.name)) },
            confirmButton = {
                TextButton(onClick = { model.delete(host.id); doomed = null }) { Text(tr("Delete"), color = Dusk.bad) }
            },
            dismissButton = { TextButton(onClick = { doomed = null }) { Text(tr("Cancel")) } },
        )
    }
}

@Composable
fun Heading(text: String, modifier: Modifier = Modifier) {
    Text(text, modifier = modifier, fontSize = 14.sp, color = Dusk.muted, style = MaterialTheme.typography.titleSmall)
}

@Composable
private fun OpenChip(session: SessionModel, onClick: () -> Unit) {
    Row(
        Modifier.clip(RoundedCornerShape(12.dp)).background(Dusk.surface)
            .border(1.dp, Dusk.edge, RoundedCornerShape(12.dp))
            .combinedClickable(onClick = onClick).padding(horizontal = 12.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Dot(statusColor(session.status))
        Spacer(Modifier.width(10.dp))
        Column {
            Text(session.spec, fontSize = 15.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(statusWord(session.status), fontSize = 12.sp, color = Dusk.muted)
        }
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun HostItem(
    host: HostRow,
    session: SessionModel?,
    onConnect: () -> Unit,
    onAnother: () -> Unit,
    onEdit: () -> Unit,
    onDelete: () -> Unit,
) {
    var menu by remember { mutableStateOf(false) }
    val tint = host.color?.let { rgb(it) } ?: Dusk.soft
    Box {
        Row(
            Modifier.fillMaxWidth().height(72.dp)
                .combinedClickable(onClick = onConnect, onLongClick = { menu = true })
                .padding(horizontal = 16.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box {
                Box(
                    Modifier.size(40.dp).clip(RoundedCornerShape(12.dp)).background(tint.copy(alpha = 0.18f)),
                    contentAlignment = Alignment.Center,
                ) { Icon(Icons.Outlined.Dns, null, tint = tint, modifier = Modifier.size(22.dp)) }
                if (session != null) {
                    Box(
                        Modifier.align(Alignment.BottomEnd).offset(3.dp, 3.dp).size(12.dp).clip(CircleShape)
                            .background(MaterialTheme.colorScheme.background).padding(2.dp).clip(CircleShape)
                            .background(statusColor(session.status)),
                    )
                }
            }
            Spacer(Modifier.width(16.dp))
            Column(Modifier.weight(1f)) {
                Text(host.name, fontSize = 16.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                val detail = session?.let { "${host.detail} · ${statusWord(it.status)}" } ?: host.detail
                Text(detail, fontSize = 14.sp, color = Dusk.muted, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            if (host.favorite) Icon(Icons.Filled.Star, null, tint = Dusk.amber, modifier = Modifier.size(18.dp))
        }
        DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
            DropdownMenuItem(text = { Text(tr("Edit")) }, onClick = { menu = false; onEdit() })
            DropdownMenuItem(text = { Text(tr("Another session")) }, onClick = { menu = false; onAnother() })
            DropdownMenuItem(text = { Text(tr("Delete"), color = Dusk.bad) }, onClick = { menu = false; onDelete() })
        }
    }
}

@Composable
fun Dot(color: Color, size: Int = 8) {
    Box(Modifier.size(size.dp).clip(CircleShape).background(color))
}

fun statusColor(s: SessionStatus): Color = when (s) {
    SessionStatus.CONNECTED -> Dusk.ok
    SessionStatus.CONNECTING -> Dusk.amber
    SessionStatus.LOST -> Dusk.lost
    SessionStatus.FAILED -> Dusk.bad
    SessionStatus.CLOSED -> Dusk.idle
}
