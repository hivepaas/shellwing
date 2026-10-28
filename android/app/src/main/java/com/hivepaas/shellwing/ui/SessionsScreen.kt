package com.hivepaas.shellwing.ui

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
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.hivepaas.shellwing.AppModel
import com.hivepaas.shellwing.SessionModel
import com.hivepaas.shellwing.statusWord
import com.hivepaas.shellwing.tr

/** Every open session, as the desktop's tab overview: its last lines, its state. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SessionsScreen(model: AppModel, onOpen: (Screen) -> Unit) {
    Scaffold(topBar = { TopAppBar(title = { Text(tr("Sessions")) }) }) { pad ->
        if (model.sessions.isEmpty()) {
            Column(
                Modifier.fillMaxSize().padding(pad).padding(32.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.Center,
            ) {
                Text(tr("No sessions open"), style = MaterialTheme.typography.titleMedium)
                Spacer(Modifier.height(8.dp))
                Text(tr("Open one from Hosts."), color = Dusk.muted)
            }
            return@Scaffold
        }
        LazyVerticalGrid(
            columns = GridCells.Adaptive(170.dp),
            modifier = Modifier.fillMaxSize().padding(pad),
            contentPadding = androidx.compose.foundation.layout.PaddingValues(16.dp),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            items(model.sessions, key = { it.id }) { s ->
                Card(s, onOpen = { onOpen(Screen.Terminal(s.id)) }, onClose = { model.close(s) })
            }
        }
    }
}

@Composable
private fun Card(session: SessionModel, onOpen: () -> Unit, onClose: () -> Unit) {
    val shape = RoundedCornerShape(16.dp)
    Column(
        Modifier.clip(shape).background(Dusk.surface).border(1.dp, Dusk.edge, shape).clickable(onClick = onOpen),
    ) {
        Box(Modifier.fillMaxWidth().height(140.dp).background(Dusk.page).padding(8.dp)) {
            val lines = preview(session)
            if (lines.isNotEmpty()) {
                Text(
                    lines.joinToString("\n"),
                    fontFamily = FontFamily.Monospace, fontSize = 7.5.sp, lineHeight = 11.sp,
                    color = Dusk.soft, maxLines = 12, softWrap = false,
                )
            } else if (!session.isLive) {
                Column(Modifier.align(Alignment.Center), horizontalAlignment = Alignment.CenterHorizontally) {
                    Text(session.ended ?: statusWord(session.status), fontSize = 12.sp, color = Dusk.muted)
                    Spacer(Modifier.height(10.dp))
                    OutlinedButton(onClick = { session.reconnect() }) { Text(tr("Connect again")) }
                }
            }
        }
        Row(Modifier.padding(start = 12.dp, end = 2.dp), verticalAlignment = Alignment.CenterVertically) {
            Dot(statusColor(session.status))
            Spacer(Modifier.width(8.dp))
            Column(Modifier.weight(1f).padding(vertical = 10.dp)) {
                Text(session.spec, fontSize = 14.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(
                    if (session.isLive) session.title else statusWord(session.status),
                    fontSize = 12.sp, color = Dusk.muted, maxLines = 1, overflow = TextOverflow.Ellipsis,
                )
            }
            IconButton(onClick = onClose) { Icon(Icons.Filled.Close, tr("Close"), tint = Dusk.muted) }
        }
    }
}

/** The frame's last lines that say something, as the card shows them. */
private fun preview(session: SessionModel): List<String> {
    val f = session.frame ?: return emptyList()
    return f.rows.map { row -> row.runs.joinToString("") { it.text }.trimEnd() }
        .dropLastWhile { it.isEmpty() }
        .takeLast(10)
}
