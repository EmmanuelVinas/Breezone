package com.breezone.app.ui.components

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.collectIsDraggedAsState
import androidx.compose.runtime.getValue
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.breezone.app.data.DeviceView
import com.breezone.app.data.Periods
import com.breezone.app.resources.Res
import com.breezone.app.resources.period_1y
import com.breezone.app.resources.period_5y
import com.breezone.app.resources.zone_comfort_30d
import com.breezone.app.resources.zone_history_24h
import com.breezone.app.resources.zone_history_7d
import org.jetbrains.compose.resources.stringResource
import com.breezone.app.data.Room
import com.breezone.app.ui.theme.Fz
import kotlin.math.ln
import kotlin.math.pow
import kotlin.math.roundToInt

/** The devices by room, in the rooms' order; those without a room last (`null`). */
fun groupByRoom(devices: List<DeviceView>, rooms: List<Room>): List<Pair<Room?, List<DeviceView>>> {
    val byRoom = rooms.sortedBy { it.position }.map { r -> r to devices.filter { it.roomId == r.id } }
    val loose = devices.filter { d -> d.roomId == null || rooms.none { it.id == d.roomId } }
    return (byRoom + (null to loose)).filter { it.second.isNotEmpty() }.map { (r, list) -> r to list.sortedBy { it.position } }
}

/** A color of the hardware (`#rrggbb`), for the display only. */
fun hexColor(hex: String?): Color? {
    val h = hex?.removePrefix("#")?.takeIf { it.length == 6 } ?: return null
    val v = h.toLongOrNull(16) ?: return null
    return Color(0xFF000000 or v)
}

/** The color of a white of `kelvin` (Tanner Helland's approximation), for the display only. */
fun kelvinColor(kelvin: Long): Color {
    val k = kelvin / 100.0
    fun c(v: Double) = v.coerceIn(0.0, 255.0).roundToInt()
    val r = if (k <= 66) 255.0 else 329.698727446 * (k - 60).pow(-0.1332047592)
    val g = if (k <= 66) 99.4708025861 * ln(k) - 161.1195681661 else 288.1221695283 * (k - 60).pow(-0.0755148492)
    val b = if (k >= 66) 255.0 else if (k <= 19) 0.0 else 138.5177312231 * ln(k - 10) - 305.0447927307
    return Color(c(r), c(g), c(b))
}

/** A rounded square with an icon, tinted when active. */
@Composable
fun IconBadge(icon: ImageVector, color: Color, active: Boolean = true, size: Dp = 40.dp) {
    Box(
        Modifier
            .size(size)
            .clip(RoundedCornerShape(size * 0.35f))
            .background(if (active) color.copy(alpha = 0.2f) else MaterialTheme.colorScheme.surfaceVariant),
        contentAlignment = Alignment.Center,
    ) {
        Icon(icon, null, Modifier.size(size * 0.5f), tint = if (active) color else Fz.colors.muted)
    }
}

/** A card of devices: a title, a count on the right, then the rows. */
@Composable
fun DeviceGroupCard(title: String, count: String?, trailing: @Composable () -> Unit = {}, content: @Composable ColumnScope.() -> Unit) {
    Column(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(24.dp))
            .background(Fz.colors.card)
            .padding(horizontal = 16.dp, vertical = 10.dp),
    ) {
        Row(Modifier.fillMaxWidth().padding(vertical = 4.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(title, style = MaterialTheme.typography.titleMedium, modifier = Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis)
            if (count != null) Text(count, fontSize = 12.sp, color = Fz.colors.muted)
            trailing()
        }
        content()
    }
}

/** A tile of the house screen: a type of device and what it does now. */
@Composable
fun TypeTile(icon: ImageVector, label: String, sub: String, color: Color, onClick: () -> Unit, modifier: Modifier = Modifier) {
    Column(
        modifier
            .clip(RoundedCornerShape(24.dp))
            .background(Fz.colors.card)
            .clickable(onClick = onClick)
            .padding(14.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        IconBadge(icon, color, size = 38.dp)
        Column {
            Text(label, style = MaterialTheme.typography.titleSmall, maxLines = 1)
            Text(sub, fontSize = 12.sp, color = Fz.colors.muted, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
    }
}

/** A window with its shutter down to what is closed: the sky shows above the slats. */
@Composable
fun ShutterWindow(open: () -> Float, modifier: Modifier = Modifier) {
    val sky = Fz.colors.cool
    val slat = Fz.colors.shutter
    val frame = MaterialTheme.colorScheme.surfaceVariant
    Canvas(modifier.clip(RoundedCornerShape(10.dp))) {
        val f = 4.dp.toPx()
        drawRect(frame)
        drawRect(Brush.verticalGradient(listOf(sky.copy(alpha = 0.7f), sky.copy(alpha = 0.25f))), Offset(f, f), Size(size.width - 2 * f, size.height - 2 * f))
        val closed = (1f - open().coerceIn(0f, 1f)) * (size.height - 2 * f)
        val step = 6.dp.toPx()
        var y = f
        while (y < f + closed) {
            val h = minOf(step - 1.5f, f + closed - y)
            drawRoundRect(slat, Offset(f, y), Size(size.width - 2 * f, h), CornerRadius(1f))
            y += step
        }
        drawRoundRect(frame, Offset.Zero, size, CornerRadius(10.dp.toPx()), style = Stroke(f))
    }
}

/** A slider showing [value] (the hardware's), moved freely; [onCommit] once released. */
@OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)
@Composable
fun FzSlider(
    value: Float,
    onCommit: (Float) -> Unit,
    range: ClosedFloatingPointRange<Float>,
    colors: androidx.compose.material3.SliderColors,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
) {
    val source = androidx.compose.runtime.remember { androidx.compose.foundation.interaction.MutableInteractionSource() }
    val dragged by source.collectIsDraggedAsState()
    val commit by androidx.compose.runtime.rememberUpdatedState(onCommit)
    val state = androidx.compose.runtime.remember(range) {
        androidx.compose.material3.SliderState(value = value.coerceIn(range), steps = 0, trackRange = range)
    }
    // The hardware's value, unless the slider is being moved.
    androidx.compose.runtime.LaunchedEffect(value, dragged) { if (!dragged) state.value = value.coerceIn(range) }
    androidx.compose.material3.Slider(
        state = state,
        onValueChange = { state.value = it },
        modifier = modifier,
        enabled = enabled,
        onValueChangeFinished = { commit(state.value) },
        colors = colors,
        interactionSource = source,
    )
}

/** "24 h", "7 j", "30 j", "1 an", "5 ans". */
@Composable
fun periodLabel(hours: Int): String = stringResource(
    when {
        hours <= Periods.DAY -> Res.string.zone_history_24h
        hours <= Periods.WEEK -> Res.string.zone_history_7d
        hours <= Periods.MONTH -> Res.string.zone_comfort_30d
        hours <= Periods.YEAR -> Res.string.period_1y
        else -> Res.string.period_5y
    },
)

/** Chips of the periods a history covers (see [Periods.available]); none when only one. */
@Composable
fun PeriodChips(hours: Int, oldest: Long?, onChange: (Int) -> Unit) {
    val periods = Periods.available(oldest, com.breezone.app.ui.nowSeconds())
    if (periods.size < 2) return
    Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        periods.forEach { h ->
            androidx.compose.material3.FilterChip(selected = hours == h, onClick = { onChange(h) }, label = { Text(periodLabel(h)) })
        }
    }
}
