package com.hivepaas.shellwing.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Fingerprint
import androidx.compose.material3.Button
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.hivepaas.shellwing.R
import com.hivepaas.shellwing.tr
import com.hivepaas.shellwing.trN

/** Over everything while locked; the sessions go on underneath. */
@Composable
fun LockScreen(sessions: Int, onUnlock: () -> Unit) {
    Column(
        Modifier.fillMaxSize().background(Dusk.field).padding(32.dp).navigationBarsPadding(),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Column(
            Modifier.weight(1f),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.Center,
        ) {
            Box(
                Modifier.size(88.dp).clip(RoundedCornerShape(26.dp)).background(Dusk.surface),
                contentAlignment = Alignment.Center,
            ) {
                Icon(painterResource(R.drawable.ic_notification), null, tint = Dusk.accent, modifier = Modifier.size(48.dp))
            }
            Spacer(Modifier.height(24.dp))
            Text(tr("shellwing is locked"), fontSize = 24.sp)
            if (sessions > 0) {
                Spacer(Modifier.height(12.dp))
                Text(
                    trN(sessions, "{n} session stays connected underneath.", "{n} sessions stay connected underneath."),
                    color = Dusk.muted,
                )
            }
        }
        Button(onClick = onUnlock, modifier = Modifier.fillMaxWidth().height(56.dp)) {
            Icon(Icons.Outlined.Fingerprint, null)
            Spacer(Modifier.size(12.dp))
            Text(tr("Unlock"), fontSize = 16.sp)
        }
        Spacer(Modifier.height(12.dp))
        Text(tr("Your fingerprint, face or screen lock unlocks it."), fontSize = 13.sp, color = Dusk.muted)
    }
}
