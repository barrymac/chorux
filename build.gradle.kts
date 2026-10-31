plugins {
	kotlin("jvm") version "2.3.21"
	kotlin("plugin.spring") version "2.3.21"
	groovy
	id("org.springframework.boot") version "4.1.2"
	id("io.spring.dependency-management") version "1.1.7"
}

group = "it.coining"
version = "0.0.5-SNAPSHOT"
description = "The fleet coordination-signal plane — chorus + x. It's just a chat room."

java {
	toolchain {
		languageVersion = JavaLanguageVersion.of(21)
	}
}

// The JP battery wire (jpEval below) carries its own main, so the bootJar's
// single-main resolution needs the room's entry point pinned loud.
springBoot {
	mainClass.set("it.coining.chorux.ChoruxApplicationKt")
}

repositories {
	mavenCentral()
}

extra["springAiVersion"] = "2.0.1"

dependencies {
	implementation("org.springframework.boot:spring-boot-starter-actuator")
	implementation("org.springframework.boot:spring-boot-starter-webmvc")
	implementation("org.jetbrains.kotlin:kotlin-reflect")
	implementation("org.springframework.ai:spring-ai-starter-mcp-server-webmvc")
	implementation("tools.jackson.module:jackson-module-kotlin")
	implementation("tools.jackson.core:jackson-databind")
	implementation("org.apache.groovy:groovy:4.0.24")
	implementation("io.micrometer:micrometer-registry-prometheus")
	// The L11 seam's first real mechanism (docs/07): a standard public XMPP
	// client library — connects to any compliant server on the planet.
	implementation("org.igniterealtime.smack:smack-java8:4.4.8")
	implementation("org.igniterealtime.smack:smack-tcp:4.4.8")
	implementation("org.igniterealtime.smack:smack-extensions:4.4.8")
	testImplementation("org.springframework.boot:spring-boot-starter-actuator-test")
	testImplementation("org.springframework.boot:spring-boot-starter-webmvc-test")
	testImplementation("org.jetbrains.kotlin:kotlin-test-junit5")
	testImplementation("org.clojure:clojure:1.12.1")
	testRuntimeOnly("org.junit.platform:junit-platform-launcher")
}

sourceSets {
	main {
		// Data-as-code (L15): the reference ledger ships in the jar as Clojure
		// source — consumable by any JVM consumer that puts Clojure on its own
		// classpath; zero boot cost here since nothing loads it at runtime.
		resources.srcDir("src/main/clojure")
	}
}

dependencyManagement {
	imports {
		mavenBom("org.springframework.ai:spring-ai-bom:${property("springAiVersion")}")
	}
}

kotlin {
	compilerOptions {
		freeCompilerArgs.addAll("-Xjsr305=strict", "-Xannotation-default-target=param-property")
	}
}

tasks.withType<Test> {
	useJUnitPlatform()
}

// The JP wire, degen-simple: JP_BATTERY=cards/<eval>/battery.json ./gradlew -q jpEval
tasks.register<JavaExec>("jpEval") {
	group = "application"
	description = "Run a pre-registered judgement battery (JP_BATTERY=path) against the System One API"
	mainClass.set("it.coining.chorux.eval.JudgementBatteryKt")
	classpath = sourceSets.main.get().runtimeClasspath
}
