const MODELS = [
  {
    value: "sonnet",
    resolvedModel: "claude-sonnet-5",
    displayName: "Claude Sonnet 5",
    supportedEffortLevels: ["low", "medium", "high", "xhigh"],
    supportsFastMode: true,
  },
];

class FakeQuery {
  constructor(prompt, options) {
    this.prompt = prompt;
    this.options = options;
    this.closed = false;
  }

  async initializationResult() {
    if (
      this.options.settingSources.length !== 0 ||
      this.options.strictMcpConfig !== true ||
      this.options.allowedTools[0] !== "mcp__agentsassemble_room__*" ||
      this.options.mcpServers.agentsassemble_room.headers.Authorization !== "Bearer fixture-token" ||
      this.options.systemPrompt.snapshot !== true
    ) {
      throw new Error("unexpected SDK options");
    }
    return { models: MODELS };
  }

  async supportedModels() {
    return MODELS;
  }

  close() {
    this.closed = true;
  }

  async *[Symbol.asyncIterator]() {
    for await (const input of this.prompt) {
      const session = this.options.sessionId ?? this.options.resume;
      yield {
        type: "system",
        subtype: "init",
        session_id: session,
        cwd: this.options.cwd,
        model: this.options.model,
        effort: this.options.effort,
        fast_mode_state: this.options.settings?.fastMode ? "on" : "off",
        permissionMode: this.options.permissionMode,
        tools: [],
        mcp_servers: [{ name: "agentsassemble_room", status: "connected" }],
      };
      yield {
        type: "result",
        subtype: "success",
        is_error: false,
        terminal_reason: "completed",
        uuid: "sdk-result-id",
        session_id: session,
        user_message_uuid: input.uuid,
        result: "fixture response",
        modelUsage: { [this.options.model]: {} },
        fast_mode_state: this.options.settings?.fastMode ? "on" : "off",
      };
    }
  }
}

export function query({ prompt, options }) {
  return new FakeQuery(prompt, options);
}
