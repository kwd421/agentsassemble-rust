import { randomUUID } from "node:crypto";

// Only live native correlation and answers live here. Rust owns room request authority.
export class OwnerRequests {
  #pending = new Map();
  #closed = false;

  constructor(state, delivery, emit, fail) {
    this.state = state;
    this.delivery = delivery;
    this.emit = emit;
    this.fail = fail;
  }

  hooks = {
    PreToolUse: [{ matcher: "^(AskUserQuestion|ExitPlanMode)$", timeout: 600, hooks: [async (input, toolUseId, { signal }) => {
      try {
        if (input.session_id !== this.state.id || input.hook_event_name !== "PreToolUse" ||
            !["AskUserQuestion", "ExitPlanMode"].includes(input.tool_name)) {
          throw new Error("invalid native question session");
        }
        const id = this.delivery.claim(input, toolUseId);
        const response = await this.ask(input.tool_name, input.tool_input, id, signal);
        return { hookSpecificOutput: {
          hookEventName: "PreToolUse", permissionDecision: response.behavior,
          ...(response.behavior === "allow" ? { updatedInput: response.updatedInput } : { permissionDecisionReason: response.message }),
        } };
      } catch (error) {
        this.fail(error);
        throw new Error("Claude native hook failed");
      }
    }] }],
  };

  canUseTool = (name, input, { requestId, signal }) => this.ask(name, input, requestId, signal);

  async ask(name, input, nativeId, signal) {
    let entry;
    let abort;
    try {
      if (this.#closed || !this.state.active || this.state.active.closing || this.#pending.size >= 128) {
        throw new Error("native request outside active turn");
      }
      if (signal.aborted) return denied();
      const mapped = mapRequest(name, input);
      const id = mapped.request.provider_request_id;
      let answer;
      entry = { turnId: this.state.active.turnId, phase: "open" };
      const resolution = new Promise((resolve) => { answer = resolve; });
      entry.answer = answer;
      entry.done = new Promise((resolve) => { entry.finish = resolve; });
      this.#pending.set(id, entry);
      abort = () => {
        if (!["open", "answered"].includes(entry.phase)) return;
        entry.phase = "cancelled";
        entry.answer(null);
        this.emit({ type: "request_cancelled", request_id: id }).catch(this.fail);
      };
      signal.addEventListener("abort", abort, { once: true });
      await this.emit({ type: "provider_request", turn_id: entry.turnId, request: mapped.request });
      const response = await resolution;
      if (this.#closed || entry.phase === "cancelled") return denied();
      if (response === null) throw new Error("missing owner response");
      const native = mapped.reply(response);
      entry.phase = "responding";
      this.delivery.expect(nativeId, (delivered) => {
        if (this.#closed) return;
        entry.phase = "delivered";
        this.emit({ type: "request_delivered", request_id: id, delivered }).catch(this.fail);
      });
      return native;
    } catch (error) {
      this.fail(error);
      throw new Error("Claude owner request failed");
    } finally {
      if (abort) signal.removeEventListener("abort", abort);
    }
  }

  accept(command) {
    if (!new Set(["request_answer", "request_complete"]).has(command.type)) return false;
    const entry = this.#pending.get(command.request_id);
    if (!entry || this.#closed) throw new Error("unknown owner response");
    if (command.type === "request_answer") {
      // A native cancellation can race a previously queued human answer. Never grant it.
      if (entry.phase === "cancelled") return true;
      if (entry.phase !== "open") throw new Error("duplicate owner response");
      entry.phase = "answered";
      entry.answer(command.resolution);
    } else {
      if (!new Set(["delivered", "cancelled"]).has(entry.phase)) throw new Error("premature owner receipt");
      this.#pending.delete(command.request_id);
      entry.finish();
    }
    return true;
  }

  async finishTurn() {
    if ([...this.#pending.values()].some((entry) => ["open", "answered"].includes(entry.phase))) {
      throw new Error("native turn ended before owner response");
    }
    await Promise.all([...this.#pending.values()].map((entry) => entry.done));
  }

  close() {
    this.#closed = true;
    for (const entry of this.#pending.values()) {
      entry.answer(null);
      entry.finish();
    }
    this.#pending.clear();
  }
}

function denied() {
  return { behavior: "deny", message: "The room participant did not authorize this action." };
}

function mapRequest(name, input) {
  if (typeof name !== "string" || !input || typeof input !== "object" || Array.isArray(input)) {
    throw new Error("invalid native tool request");
  }
  const request = {
    provider_request_id: randomUUID(), request_kind: "permission",
    title: name === "ExitPlanMode" ? "Claude requests plan approval" : "Claude requests permission",
    description: name === "ExitPlanMode" ? "Claude wants to leave plan mode and start work." : name,
    timeout_seconds: 600,
    prompt: { response_kind: "option", options: [
      { id: "allow-once", label: "Allow once", kind: "allow_once", description: "" },
      { id: "deny", label: "Deny", kind: "reject_once", description: "" },
    ] },
  };
  if (name !== "AskUserQuestion") {
    return { request, reply: (resolution) => {
      if (resolution.response_kind !== "option" || !["allow-once", "deny"].includes(resolution.option_id)) {
        throw new Error("invalid permission answer");
      }
      return resolution.option_id === "allow-once" ? { behavior: "allow", updatedInput: input } : denied();
    } };
  }
  if (!Array.isArray(input.questions) || input.questions.length < 1 || input.questions.length > 3) {
    throw new Error("invalid native questions");
  }
  request.request_kind = "user_input";
  request.title = "Claude needs your answer";
  request.description = "Answer these questions to continue.";
  request.prompt = { response_kind: "answers", questions: input.questions.map((question, index) => ({
    id: `question-${index}`, header: question.header ?? "", question: question.question,
    multiple: question.multiSelect === true, is_other: true, is_secret: false,
    options: (question.options ?? []).map((option, index) => ({
      id: `option-${index}`, label: option.label, kind: "answer", description: option.description ?? "",
    })),
  })) };
  return { request, reply: (resolution) => {
    if (resolution.response_kind !== "answers") throw new Error("invalid question answer");
    const answers = Object.fromEntries(input.questions.map((question, index) => {
      const values = resolution.answers?.[`question-${index}`];
      if (!Array.isArray(values) || !values.length || values.some((value) => typeof value !== "string" || !value.trim())) {
        throw new Error("missing question answer");
      }
      return [question.question, values.join(", ")];
    }));
    return { behavior: "allow", updatedInput: { ...input, answers } };
  } };
}
