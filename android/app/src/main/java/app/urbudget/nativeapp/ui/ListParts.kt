package app.urbudget.nativeapp.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.KeyboardArrowDown
import androidx.compose.material.icons.automirrored.outlined.KeyboardArrowRight
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import androidx.compose.ui.res.stringResource
import app.urbudget.nativeapp.R

/**
 * One row of a card split across lazy-list items, so long lists stay virtualized: the first row
 * takes the card's top corners and the last its bottom ones.
 */
internal fun Modifier.cardSegment(index: Int, count: Int, fill: Color): Modifier {
    val top = if (index == 0) 18.dp else 0.dp
    val bottom = if (index == count - 1) 18.dp else 0.dp
    return this.fillMaxWidth().clip(RoundedCornerShape(topStart = top, topEnd = top, bottomStart = bottom, bottomEnd = bottom)).background(fill)
}

/** A card's fill: white on the grey page, grey on a sheet's white. */
@Composable
internal fun cardFill(): Color = if (LocalSheet.current) ur.surface2 else ur.surface

/**
 * A display model built on a worker from `keys`. While a new one builds, the previous one stays
 * on screen; null only until the first is ready.
 */
@Composable
internal fun <T> rememberPrepared(vararg keys: Any?, build: suspend () -> T): T? {
    val latest by rememberUpdatedState(build)
    val result by produceState<T?>(null, *keys) { value = withContext(Dispatchers.Default) { latest() } }
    return result
}

/** A count in a small accent pill, as the desktop's account badges show transactions to approve. */
@Composable
internal fun CountBadge(count: Int, modifier: Modifier = Modifier) {
    Box(modifier.heightIn(min = 20.dp).widthIn(min = 20.dp).clip(RoundedCornerShape(10.dp)).background(ur.accent).padding(horizontal = 6.dp),
        contentAlignment = Alignment.Center) {
        Text(if (count > 99) "99+" else "$count", color = if (ur.dark) ur.bg else Color.White, fontSize = 12.sp, fontWeight = FontWeight.Bold, maxLines = 1)
    }
}

/** The yellow dot that marks spending still needing a category. */
@Composable
internal fun AttentionDot(modifier: Modifier = Modifier) {
    Box(modifier.size(9.dp).clip(CircleShape).background(ur.attention))
}

/** A section heading that folds its rows away, with a figure at the end. */
@Composable
internal fun FoldingHeader(title: String, open: Boolean, onToggle: () -> Unit, trailing: String? = null, trailingColor: Color = ur.text) {
    Row(Modifier.fillMaxWidth().clip(RoundedCornerShape(10.dp)).clickable(role = Role.Button, onClickLabel = if (open) stringResource(R.string.fold_collapse, title) else stringResource(R.string.fold_expand, title), onClick = onToggle)
        .padding(top = 10.dp, bottom = 2.dp, start = 4.dp, end = 4.dp), verticalAlignment = Alignment.CenterVertically) {
        Icon(if (open) Icons.Outlined.KeyboardArrowDown else Icons.AutoMirrored.Outlined.KeyboardArrowRight, null, Modifier.size(22.dp), tint = ur.muted)
        Text(title, Modifier.weight(1f).padding(start = 4.dp), fontSize = 20.sp, fontWeight = FontWeight.Bold, color = ur.text)
        if (trailing != null) Text(trailing, fontSize = 17.sp, fontWeight = FontWeight.Bold, style = Tabular, color = trailingColor, maxLines = 1)
    }
}
