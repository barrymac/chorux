# chorux — coordination messaging service
# Build:  docker build -t chorux .
# The service exposes MCP (streamable HTTP) on :8080.

FROM eclipse-temurin:21-jdk AS build
WORKDIR /workspace
COPY gradlew gradlew
COPY gradle gradle
COPY build.gradle.kts settings.gradle.kts gradle.properties* ./
RUN ./gradlew --no-daemon dependencies -q >/dev/null 2>&1 || true
COPY src src
RUN ./gradlew --no-daemon bootJar -x test -q

FROM eclipse-temurin:21-jre
WORKDIR /app
RUN useradd -r -u 10001 chorux
COPY --from=build /workspace/build/libs/*.jar /app/chorux.jar
USER chorux
EXPOSE 8080
ENTRYPOINT ["java", "-jar", "/app/chorux.jar"]
