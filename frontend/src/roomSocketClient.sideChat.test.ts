import { describe, expect, it, vi } from "vitest";
import { event, flushPromises, handshakeFrames, openHarness } from "./test/roomSocketHarness";
import { parseSideChatSnapshot } from "./lib/sideChatContract";

const generation = "10000000-0000-4000-8000-000000000001";
const roomUid = "30000000-0000-4000-8000-000000000003";
function update(seq = 57) {
  return {room_id:"general", generation, retained_after_seq:seq - 1,
    message:{id:"20000000-0000-4000-8000-000000000002",seq, created_at:"2026-09-08T00:00:00Z",participant_id:"operator-local",display_name:"Host",content:"private text"}};
}

describe("independent ephemeral socket stream", () => {
  it("delivers side chat without advancing the durable cursor and binds its own ACK", async () => {
    const onSideChat = vi.fn();
    const onRoomEvents = vi.fn();
    const { handle, sockets, opened } = openHarness({onSideChat,onRoomEvents}, ["room_events","side_chat"]);
    await flushPromises();
    sockets[0].open();
    const handshake = handshakeFrames(0,0,(receipt) => {receipt.streams=["room_events","side_chat"];});
    sockets[0].receive(handshake.receipt);
    sockets[0].receiveRaw(handshake.rawSnapshot);
    await opened;
    sockets[0].receive({op:"side_chat_updated",update:update()});
    sockets[0].receive({op:"event",stream:"room_events",events:[event(1)],latest_seq:1});
    await flushPromises();
    expect(onSideChat).toHaveBeenCalledWith(update());
    expect(onRoomEvents).toHaveBeenCalledWith([event(1)]);
    expect(handle.ready()).toBe(true);
    const pending = handle.command("side_chat.send", {generation, after_seq:57, content:"private text"});
    const command = sockets[0].sent.at(-1)!;
    sockets[0].receive({op:"ack",accepted:true,resolution:"committed",request_id:command.request_id,action:"side_chat.send",result:{update:update(58)}});
    await expect(pending).resolves.toMatchObject({result:{update:update(58)}});
    handle.close();
  });

  it("rejects unsolicited private delivery and cross-room or discontinuous snapshots", async () => {
    const onSideChat = vi.fn();
    const onError = vi.fn();
    const {handle,sockets,opened} = openHarness({onSideChat,onError});
    await flushPromises();
    sockets[0].open();
    const handshake = handshakeFrames(0,0);
    sockets[0].receive(handshake.receipt);
    sockets[0].receiveRaw(handshake.rawSnapshot);
    await opened;
    sockets[0].receive({op:"side_chat_updated",update:update()});
    await flushPromises();
    expect(onSideChat).not.toHaveBeenCalled();
    expect(onError).toHaveBeenCalledWith(expect.objectContaining({category:"unexpected_stream"}));
    expect(handle.ready()).toBe(false);
    handle.close();
    const snapshot = {room_id:"general",room_uid:roomUid,generation,retained_after_seq:56,latest_seq:57,messages:[update().message]};
    expect(parseSideChatSnapshot(snapshot,"general",roomUid)).toEqual(snapshot);
    expect(() => parseSideChatSnapshot(snapshot,"other",roomUid)).toThrow();
    expect(() => parseSideChatSnapshot(snapshot,"general",generation)).toThrow();
    expect(() => parseSideChatSnapshot({...snapshot,retained_after_seq:55},"general",roomUid)).toThrow();
  });
});
