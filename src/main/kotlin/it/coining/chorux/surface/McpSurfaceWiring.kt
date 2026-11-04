package it.coining.chorux.surface

import it.coining.chorux.room.RoomRegistry
import io.modelcontextprotocol.spec.HttpHeaders
import org.springframework.ai.mcp.server.webmvc.transport.WebMvcStreamableServerTransportProvider
import org.springframework.ai.tool.method.MethodToolCallbackProvider
import org.springframework.context.annotation.Bean
import org.springframework.context.annotation.Configuration
import org.springframework.http.HttpMethod
import org.springframework.web.servlet.function.HandlerFunction
import org.springframework.web.servlet.function.RouterFunction
import org.springframework.web.servlet.function.ServerResponse

/**
 * MCP wiring — the adoption wedge (docs/03-mcp-first-adoption.md).
 * Spring AI 2 turns every [Tool]-annotated method into an MCP tool; the
 * surface stays three tools wide and everything else is nudges.
 */
@Configuration
class McpSurfaceWiring {
    @Bean
    fun chatRoomToolCallbacks(tools: ChatRoomTools): MethodToolCallbackProvider =
        MethodToolCallbackProvider.builder().toolObjects(tools).build()

    /**
     * Connection end is a seat fact (docs/02: Left is seat release). MCP session
     * DELETE is the disconnect hook: the transport provider calls
     * removeSession(sessionId) on DELETE — this bean is the same router the
     * autoconfigure would mint (name-pinned, so the autoconfigure backs off),
     * wrapped to observe the DELETE's session id and release every seat that
     * connection claimed, Left per docs/02. Claims are connection-scoped (L19),
     * so no lease machinery is needed: the wire's own end IS the release.
     */
    @Bean
    fun webMvcStreamableServerRouterFunction(
        provider: WebMvcStreamableServerTransportProvider,
        registry: RoomRegistry,
    ): RouterFunction<ServerResponse> {
        val delegate = provider.routerFunction
        return RouterFunction { request ->
            delegate.route(request).map { handler ->
                if (request.method() != HttpMethod.DELETE) handler
                else HandlerFunction { req ->
                    val response = handler.handle(req)
                    req.headers().firstHeader(HttpHeaders.MCP_SESSION_ID)?.let(registry::release)
                    response
                }
            }
        }
    }
}
