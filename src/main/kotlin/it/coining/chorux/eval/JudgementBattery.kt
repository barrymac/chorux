package it.coining.chorux.eval

import java.net.URI
import java.net.http.HttpClient
import java.net.http.HttpRequest
import java.net.http.HttpResponse
import java.nio.file.Path
import java.time.Duration
import kotlin.io.path.readText
import tools.jackson.databind.JsonNode
import tools.jackson.module.kotlin.jacksonObjectMapper

/**
 * Pre-registered judgement battery runner (the JP as a backend wire, no
 * ceremony). The battery JSON carries the full System One request:
 *
 *   { "model": "jev-latest", "state": { ... }, "questions": { ... } }
 *
 * Composition law (docs-adjacent): every Choice carries an escape option,
 * every Score a cannot-judge level, oracles (measured facts) live in state,
 * never in the questions. The runner enforces none of that — the battery
 * file is the pre-registration; this is just the wire.
 *
 *   JP_BATTERY=path/to/battery.json JEV_API_KEY=... ./gradlew -q jpEval
 *
 * Fail-loud everywhere: no battery path, no key, no silent guess at a
 * scenario — missing config exits before any byte leaves the machine.
 */
fun main() {
	val batteryPath = System.getenv("JP_BATTERY")
		?: error("JP_BATTERY unset — point it at a pre-registered battery JSON; refusing to guess a scenario")
	val apiKey = System.getenv("JEV_API_KEY") ?: System.getenv("TYPESAFE_API_KEY")
		?: error("JEV_API_KEY/TYPESAFE_API_KEY unset — no judgement-plane credential, refusing loudly")
	val baseUrl = System.getenv("JEV_API_URL") ?: "https://api.typesafe.ai"

	val mapper = jacksonObjectMapper()
	val battery = mapper.readTree(Path.of(batteryPath).readText())
		?: error("battery at $batteryPath parsed to nothing — refusing empty judgement")
	require(battery.has("state") && battery.has("questions")) {
		"battery at $batteryPath lacks state/questions — a judgement with no ground is astrology, refusing"
	}

	val request = mapper.createObjectNode().apply {
		put("model", battery.get("model")?.takeIf { it.isTextual }?.asText() ?: "jev-latest")
		set("state", battery.get("state"))
		set("questions", battery.get("questions"))
	}

	val http = HttpClient.newBuilder().connectTimeout(Duration.ofSeconds(15)).build()
	val httpRequest = HttpRequest.newBuilder(URI.create("$baseUrl/v1/systemone"))
		.timeout(Duration.ofSeconds(180))
		.header("Authorization", "Bearer $apiKey")
		.header("Content-Type", "application/json")
		.POST(HttpRequest.BodyPublishers.ofString(mapper.writeValueAsString(request)))
		.build()

	val response = http.send(httpRequest, HttpResponse.BodyHandlers.ofString())
	check(response.statusCode() == 200) {
		"systemone refused HTTP ${response.statusCode()}: ${response.body().take(500)}"
	}
	println(mapper.writerWithDefaultPrettyPrinter().writeValueAsString(mapper.readTree(response.body())))
}
