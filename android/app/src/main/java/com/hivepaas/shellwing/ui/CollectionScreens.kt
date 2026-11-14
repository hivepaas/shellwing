package com.hivepaas.shellwing.ui

import androidx.compose.foundation.layout.Arrangement
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
import androidx.compose.material.icons.outlined.Key
import androidx.compose.material3.Button
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.hivepaas.shellwing.AppModel
import com.hivepaas.shellwing.tr

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
