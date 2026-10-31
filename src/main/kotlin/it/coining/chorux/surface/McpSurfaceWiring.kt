package it.coining.chorux.surface

import org.springframework.ai.tool.method.MethodToolCallbackProvider
import org.springframework.context.annotation.Bean
import org.springframework.context.annotation.Configuration

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
}
