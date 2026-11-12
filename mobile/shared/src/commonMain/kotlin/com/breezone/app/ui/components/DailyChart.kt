package com.breezone.app.ui.components

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import com.breezone.app.data.DayStats
import com.breezone.app.resources.Res
import com.breezone.app.resources.zone_comfort_legend_avg
import com.breezone.app.resources.zone_comfort_legend_range
import com.breezone.app.resources.zone_comfort_share
import com.breezone.app.resources.zone_demand
import com.breezone.app.resources.zone_humidity_label
import com.breezone.app.ui.airQualityColor
import com.breezone.app.ui.airQualityLabel
import com.breezone.app.ui.formatTemp
import com.breezone.app.ui.theme.Fz
import com.patrykandpatrick.vico.multiplatform.cartesian.CartesianChartHost
import com.patrykandpatrick.vico.multiplatform.cartesian.Zoom
import com.patrykandpatrick.vico.multiplatform.cartesian.axis.Axis
import com.patrykandpatrick.vico.multiplatform.cartesian.axis.HorizontalAxis
import com.patrykandpatrick.vico.multiplatform.cartesian.axis.VerticalAxis
import com.patrykandpatrick.vico.multiplatform.cartesian.data.CartesianChartModelProducer
import com.patrykandpatrick.vico.multiplatform.cartesian.data.CartesianLayerRangeProvider
import com.patrykandpatrick.vico.multiplatform.cartesian.data.CartesianValueFormatter
import com.patrykandpatrick.vico.multiplatform.cartesian.data.CandlestickCartesianLayerModel
import com.patrykandpatrick.vico.multiplatform.cartesian.data.candlestickSeries
import com.patrykandpatrick.vico.multiplatform.cartesian.data.columnSeries
import com.patrykandpatrick.vico.multiplatform.cartesian.data.lineSeries
import com.patrykandpatrick.vico.multiplatform.cartesian.layer.CandlestickCartesianLayer
import com.patrykandpatrick.vico.multiplatform.cartesian.layer.ColumnCartesianLayer
import com.patrykandpatrick.vico.multiplatform.cartesian.layer.LineCartesianLayer
import com.patrykandpatrick.vico.multiplatform.cartesian.layer.rememberCandlestickCartesianLayer
import com.patrykandpatrick.vico.multiplatform.cartesian.layer.rememberColumnCartesianLayer
import com.patrykandpatrick.vico.multiplatform.cartesian.layer.rememberLine
import com.patrykandpatrick.vico.multiplatform.cartesian.layer.rememberLineCartesianLayer
import com.patrykandpatrick.vico.multiplatform.cartesian.rememberCartesianChart
import com.patrykandpatrick.vico.multiplatform.cartesian.rememberVicoZoomState
import com.patrykandpatrick.vico.multiplatform.common.Fill
import com.patrykandpatrick.vico.multiplatform.common.component.rememberLineComponent
import com.patrykandpatrick.vico.multiplatform.common.data.ExtraStore
import org.jetbrains.compose.resources.stringResource
import kotlin.math.ceil
import kotlin.math.floor
import kotlin.math.roundToInt

/** "dd/MM" of a "YYYY-MM-DD" day. */
private fun shortDay(day: String): String = day.split('-').let { "${it.getOrElse(2) { "" }}/${it.getOrElse(1) { "" }}" }

@Composable
private fun dayAxis(days: List<DayStats>) = HorizontalAxis.rememberBottom(
    label = rememberAxisLabel(),
    guideline = null,
    valueFormatter = remember(days) { CartesianValueFormatter { _, x, _ -> days.getOrNull(x.roundToInt())?.let { shortDay(it.day) }.orEmpty() } },
    itemPlacer = remember(days.size) { HorizontalAxis.ItemPlacer.aligned(spacing = { if (days.size > 10) 5 else 1 }) },
)

/** One bar per day of the given color, rounded. */
private class SingleCandle(private val candle: CandlestickCartesianLayer.Candle) : CandlestickCartesianLayer.CandleProvider {
    override fun getCandle(entry: CandlestickCartesianLayerModel.Entry, extraStore: ExtraStore) = candle
    override fun getWidestCandle(extraStore: ExtraStore) = candle
}

/** Day by day: min–max temperature (bars), mean temperature, mean humidity (right axis). */
@Composable
fun DailyTemperatureChart(days: List<DayStats>, modifier: Modifier = Modifier) {
    val withTemp = days.withIndex().filter { (_, d) -> d.minTemp != null && d.maxTemp != null }
    val humidity = days.withIndex().filter { (_, d) -> d.avgHumidity != null }
    val producer = remember { CartesianChartModelProducer() }
    LaunchedEffect(days) {
        if (withTemp.isEmpty()) return@LaunchedEffect
        producer.runTransaction {
            val x = withTemp.map { it.index }
            candlestickSeries(
                x = x,
                opening = withTemp.map { it.value.minTemp!! },
                closing = withTemp.map { it.value.maxTemp!! },
                low = withTemp.map { it.value.minTemp!! },
                high = withTemp.map { it.value.maxTemp!! },
            )
            lineSeries { series(x, withTemp.map { it.value.avgTemp ?: it.value.minTemp!! }) }
            if (humidity.isNotEmpty()) lineSeries { series(humidity.map { it.index }, humidity.map { it.value.avgHumidity!! }) }
        }
    }
    if (withTemp.isEmpty()) return

    val heat = Fz.colors.heat
    val cool = Fz.colors.cool
    val candle = CandlestickCartesianLayer.Candle(
        body = rememberLineComponent(Fill(heat.copy(alpha = 0.3f)), thickness = 14.dp, shape = RoundedCornerShape(6.dp)),
        topWick = rememberLineComponent(Fill(Color.Transparent), thickness = 0.dp),
    )
    val mean = LineCartesianLayer.rememberLine(fill = LineCartesianLayer.LineFill.single(Fill(heat)))
    val meanHumidity = LineCartesianLayer.rememberLine(
        fill = LineCartesianLayer.LineFill.single(Fill(cool)),
        stroke = LineCartesianLayer.LineStroke.Dashed(thickness = 1.5.dp),
    )
    val range = stringResource(Res.string.zone_comfort_legend_range)
    val avg = stringResource(Res.string.zone_comfort_legend_avg)
    val humidityLabel = stringResource(Res.string.zone_humidity_label)
    val marker = rememberTextMarker(days, range, avg, humidityLabel) { x ->
        val d = days.getOrNull(x.roundToInt()) ?: return@rememberTextMarker ""
        buildString {
            append(shortDay(d.day))
            append("\n$range ${formatTemp(d.minTemp)} – ${formatTemp(d.maxTemp)}")
            d.avgTemp?.let { append("\n$avg ${formatTemp(it)}") }
            d.avgHumidity?.let { append("\n$humidityLabel ${it.roundToInt()} %") }
        }
    }
    // The scale follows the temperatures instead of starting at 0°.
    val lo = floor(withTemp.minOf { it.value.minTemp!! }) - 1
    val hi = ceil(withTemp.maxOf { it.value.maxTemp!! }) + 1
    val temperatureRange = remember(lo, hi) { CartesianLayerRangeProvider.fixed(minY = lo, maxY = hi) }
    val layers = buildList {
        add(
            rememberCandlestickCartesianLayer(
                candleProvider = remember(candle) { SingleCandle(candle) },
                rangeProvider = temperatureRange,
                verticalAxisPosition = Axis.Position.Vertical.Start,
            ),
        )
        add(
            rememberLineCartesianLayer(
                lineProvider = remember(mean) { LineCartesianLayer.LineProvider.series(mean) },
                rangeProvider = temperatureRange,
                verticalAxisPosition = Axis.Position.Vertical.Start,
            ),
        )
        if (humidity.isNotEmpty()) {
            add(
                rememberLineCartesianLayer(
                    lineProvider = remember(meanHumidity) { LineCartesianLayer.LineProvider.series(meanHumidity) },
                    rangeProvider = remember { CartesianLayerRangeProvider.fixed(minY = 0.0, maxY = 100.0) },
                    verticalAxisPosition = Axis.Position.Vertical.End,
                ),
            )
        }
    }
    val axisLabel = rememberAxisLabel()
    CartesianChartHost(
        chart = rememberCartesianChart(
            *layers.toTypedArray(),
            startAxis = VerticalAxis.rememberStart(
                label = axisLabel,
                line = null,
                tick = null,
                valueFormatter = remember { CartesianValueFormatter { _, y, _ -> "${y.toInt()}°" } },
            ),
            endAxis = if (humidity.isNotEmpty()) {
                VerticalAxis.rememberEnd(
                    label = axisLabel,
                    line = null,
                    tick = null,
                    guideline = null,
                    valueFormatter = remember { CartesianValueFormatter { _, y, _ -> "${y.toInt()}%" } },
                    itemPlacer = remember { VerticalAxis.ItemPlacer.step({ 25.0 }) },
                )
            } else {
                null
            },
            bottomAxis = dayAxis(days),
            marker = marker,
        ),
        modelProducer = producer,
        modifier = modifier.fillMaxWidth().height(170.dp),
        zoomState = rememberVicoZoomState(zoomEnabled = true, initialZoom = Zoom.Content),
        animateIn = false,
    )
}

/** Day by day: share of the time comfortable (bars) and hours in demand (right axis), if recorded. */
@Composable
fun DailyComfortChart(days: List<DayStats>, modifier: Modifier = Modifier) {
    val demand = days.withIndex().filter { (_, d) -> d.demandSecs != null }
    val producer = remember { CartesianChartModelProducer() }
    LaunchedEffect(days) {
        if (days.isEmpty()) return@LaunchedEffect
        producer.runTransaction {
            columnSeries { series(days.indices.toList(), days.map { (it.comfort ?: 0.0) * 100 }) }
            if (demand.isNotEmpty()) lineSeries { series(demand.map { it.index }, demand.map { it.value.demandSecs!! / 3600.0 }) }
        }
    }
    if (days.isEmpty()) return

    val primary = MaterialTheme.colorScheme.primary
    val heat = Fz.colors.heat
    val column = rememberLineComponent(Fill(primary), thickness = 12.dp, shape = RoundedCornerShape(topStart = 4.dp, topEnd = 4.dp))
    val demandLine = LineCartesianLayer.rememberLine(fill = LineCartesianLayer.LineFill.single(Fill(heat)))
    val comfortLabel = stringResource(Res.string.zone_comfort_share)
    val demandLabel = stringResource(Res.string.zone_demand)
    val marker = rememberTextMarker(days, comfortLabel, demandLabel) { x ->
        val d = days.getOrNull(x.roundToInt()) ?: return@rememberTextMarker ""
        buildString {
            append(shortDay(d.day))
            d.comfort?.let { append("\n$comfortLabel ${(it * 100).roundToInt()} %") }
            d.demandSecs?.let { append("\n$demandLabel ${it / 3600} h ${((it % 3600) / 60).toString().padStart(2, '0')}") }
        }
    }
    val layers = buildList {
        add(
            rememberColumnCartesianLayer(
                columnProvider = remember(column) { ColumnCartesianLayer.ColumnProvider.series(column) },
                rangeProvider = remember { CartesianLayerRangeProvider.fixed(minY = 0.0, maxY = 100.0) },
                verticalAxisPosition = Axis.Position.Vertical.Start,
            ),
        )
        if (demand.isNotEmpty()) {
            add(
                rememberLineCartesianLayer(
                    lineProvider = remember(demandLine) { LineCartesianLayer.LineProvider.series(demandLine) },
                    rangeProvider = remember { CartesianLayerRangeProvider.fixed(minY = 0.0) },
                    verticalAxisPosition = Axis.Position.Vertical.End,
                ),
            )
        }
    }
    val axisLabel = rememberAxisLabel()
    CartesianChartHost(
        chart = rememberCartesianChart(
            *layers.toTypedArray(),
            startAxis = VerticalAxis.rememberStart(
                label = axisLabel,
                line = null,
                tick = null,
                valueFormatter = remember { CartesianValueFormatter { _, y, _ -> "${y.toInt()}%" } },
                    itemPlacer = remember { VerticalAxis.ItemPlacer.step({ 25.0 }) },
            ),
            endAxis = if (demand.isNotEmpty()) {
                VerticalAxis.rememberEnd(
                    label = axisLabel,
                    line = null,
                    tick = null,
                    guideline = null,
                    valueFormatter = remember { CartesianValueFormatter { _, y, _ -> "${y.toInt()} h" } },
                    itemPlacer = remember { VerticalAxis.ItemPlacer.step({ 1.0 }) },
                )
            } else {
                null
            },
            bottomAxis = dayAxis(days),
            marker = marker,
        ),
        modelProducer = producer,
        modifier = modifier.fillMaxWidth().height(140.dp),
        zoomState = rememberVicoZoomState(zoomEnabled = true, initialZoom = Zoom.Content),
        animateIn = false,
    )
}

/** Day by day: share of good, regular and bad air quality, stacked. */
@Composable
fun AirQualityDaysChart(days: List<DayStats>, modifier: Modifier = Modifier) {
    val measured = days.withIndex().filter { (_, d) -> d.airQualitySecs?.let { it.good + it.regular + it.bad > 0 } == true }
    val producer = remember { CartesianChartModelProducer() }
    LaunchedEffect(days) {
        if (measured.isEmpty()) return@LaunchedEffect
        producer.runTransaction {
            columnSeries {
                val x = measured.map { it.index }
                fun share(pick: (com.breezone.app.data.AirQualitySecs) -> Long) = measured.map { (_, d) ->
                    val s = d.airQualitySecs!!
                    pick(s) * 100.0 / (s.good + s.regular + s.bad)
                }
                series(x, share { it.good })
                series(x, share { it.regular })
                series(x, share { it.bad })
            }
        }
    }
    if (measured.isEmpty()) return

    val levels = listOf("good", "regular", "bad")
    val columns = levels.map { level ->
        rememberLineComponent(Fill(airQualityColor(level)), thickness = 12.dp)
    }
    val names = levels.map { airQualityLabel(it) }
    val marker = rememberTextMarker(days, names) { x ->
        val d = days.getOrNull(x.roundToInt()) ?: return@rememberTextMarker ""
        val s = d.airQualitySecs ?: return@rememberTextMarker shortDay(d.day)
        val total = (s.good + s.regular + s.bad).coerceAtLeast(1)
        buildString {
            append(shortDay(d.day))
            listOf(s.good, s.regular, s.bad).forEachIndexed { i, secs -> append("\n${names[i]} ${(secs * 100 / total)} %") }
        }
    }
    val axisLabel = rememberAxisLabel()
    CartesianChartHost(
        chart = rememberCartesianChart(
            rememberColumnCartesianLayer(
                columnProvider = remember(columns) { ColumnCartesianLayer.ColumnProvider.series(columns) },
                mergeMode = { ColumnCartesianLayer.MergeMode.Stacked },
                rangeProvider = remember { CartesianLayerRangeProvider.fixed(minY = 0.0, maxY = 100.0) },
            ),
            startAxis = VerticalAxis.rememberStart(
                label = axisLabel,
                line = null,
                tick = null,
                valueFormatter = remember { CartesianValueFormatter { _, y, _ -> "${y.toInt()}%" } },
                    itemPlacer = remember { VerticalAxis.ItemPlacer.step({ 25.0 }) },
            ),
            bottomAxis = dayAxis(days),
            marker = marker,
        ),
        modelProducer = producer,
        modifier = modifier.fillMaxWidth().height(130.dp),
        zoomState = rememberVicoZoomState(zoomEnabled = true, initialZoom = Zoom.Content),
        animateIn = false,
    )
}
