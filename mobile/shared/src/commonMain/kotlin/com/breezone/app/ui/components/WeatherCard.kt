package com.breezone.app.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.breezone.app.data.WeatherView
import com.breezone.app.resources.Res
import com.breezone.app.resources.weather_clear
import com.breezone.app.resources.weather_cloudy
import com.breezone.app.resources.weather_drizzle
import com.breezone.app.resources.weather_fog
import com.breezone.app.resources.weather_max_min
import com.breezone.app.resources.weather_source
import com.breezone.app.resources.weather_rain
import com.breezone.app.resources.weather_snow
import com.breezone.app.resources.weather_spells
import com.breezone.app.resources.weather_storm
import com.breezone.app.ui.clock
import com.breezone.app.ui.icons.FzIcons
import com.breezone.app.ui.theme.Fz
import org.jetbrains.compose.resources.StringResource
import org.jetbrains.compose.resources.stringResource
import kotlin.math.roundToInt

/** The look of a WMO weather code: an icon and a few words. */
private fun look(code: Long?): Pair<ImageVector, StringResource> {
    val c = code ?: 0
    return when {
        c == 0L -> FzIcons.Sun to Res.string.weather_clear
        c <= 2 -> FzIcons.CloudSun to Res.string.weather_spells
        c == 3L -> FzIcons.Cloud to Res.string.weather_cloudy
        c <= 48 -> FzIcons.CloudFog to Res.string.weather_fog
        c <= 57 -> FzIcons.CloudDrizzle to Res.string.weather_drizzle
        c <= 67 || c in 80..82 -> FzIcons.CloudRain to Res.string.weather_rain
        c <= 77 || c == 85L || c == 86L -> FzIcons.CloudSnow to Res.string.weather_snow
        else -> FzIcons.CloudLightning to Res.string.weather_storm
    }
}

private fun deg(v: Double?) = if (v == null) "–" else "${v.roundToInt()}°"

/** The weather of the home: now, today, the sun and the next hours (nothing without a city). */
@Composable
fun WeatherCard(view: WeatherView?, modifier: Modifier = Modifier) {
    val w = view?.weather ?: return
    val (icon, label) = look(w.code)
    Column(
        modifier.fillMaxWidth().clip(RoundedCornerShape(24.dp)).background(Fz.colors.card).padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Icon(icon, stringResource(label), Modifier.size(44.dp), tint = Fz.colors.cool)
            Spacer(Modifier.width(14.dp))
            Column(Modifier.weight(1f)) {
                Text(w.place, fontSize = 12.sp, color = Fz.colors.muted, maxLines = 1)
                Text(deg(w.temperature), fontSize = 34.sp, fontWeight = FontWeight.SemiBold)
                Text(stringResource(Res.string.weather_max_min, stringResource(label), deg(w.todayMax), deg(w.todayMin)), fontSize = 12.sp, color = Fz.colors.muted)
            }
            val sun = view.sun
            if (sun?.sunrise != null && sun.sunset != null) {
                Column(horizontalAlignment = Alignment.End, verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Icon(FzIcons.Sunrise, null, Modifier.size(14.dp), tint = Fz.colors.warning)
                        Spacer(Modifier.width(4.dp))
                        Text(clock(sun.sunrise), fontSize = 12.sp)
                    }
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Icon(FzIcons.Sunset, null, Modifier.size(14.dp), tint = Fz.colors.warning)
                        Spacer(Modifier.width(4.dp))
                        Text(clock(sun.sunset), fontSize = 12.sp)
                    }
                }
            }
        }
        Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(16.dp)) {
            w.hours.filterIndexed { i, _ -> i % 3 == 0 }.take(8).forEach { h ->
                val (hIcon, hLabel) = look(h.code)
                Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    Text(clock(h.ts), fontSize = 11.sp, color = Fz.colors.muted)
                    Icon(hIcon, stringResource(hLabel), Modifier.size(20.dp))
                    Text(deg(h.temperature), fontSize = 12.sp, fontWeight = FontWeight.Medium)
                }
            }
        }
        Text(stringResource(Res.string.weather_source), fontSize = 10.sp, color = Fz.colors.muted, modifier = Modifier.align(Alignment.End))
    }
}
