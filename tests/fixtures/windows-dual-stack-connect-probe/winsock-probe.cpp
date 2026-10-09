// Diagnostic only. Connects to the owned media fixture without sending data.
#define WIN32_LEAN_AND_MEAN
#include <winsock2.h>
#include <ws2tcpip.h>
#include <mstcpip.h>
#include <windows.h>
#include <array>
#include <chrono>
#include <cstdio>
#include <cstring>
#pragma comment(lib, "Ws2_32.lib")

namespace {
using Clock = std::chrono::steady_clock;
Clock::time_point started;
constexpr int kGroups = 40, kLanes = 6, kFallbackMs = 300, kGroupMs = 700;
constexpr int kWorkMs = 30000, kCleanupMs = 35000;
unsigned starts = 0, successes = 0, refusals = 0, unhandled = 0, readyAtClose = 0;
unsigned opened = 0, closed = 0, eventsOpened = 0, eventsClosed = 0;
unsigned failures = 0, operations = 0, maxLive = 0;
bool randomize = false;
long long elapsed() { return std::chrono::duration_cast<std::chrono::milliseconds>(Clock::now() - started).count(); }
void record(unsigned id, const char* stage, int result, int error, long long at = -1) {
  ++operations;
  std::printf("{\"type\":\"operation\",\"id\":%u,\"stage\":\"%s\",\"result\":%d,\"error\":%d,\"ms\":%lld}\n", id, stage, result, error, at < 0 ? elapsed() : at);
}
void fail(unsigned id, const char* stage, int error, long long at = -1) {
  ++failures;
  std::printf("{\"type\":\"failure\",\"id\":%u,\"stage\":\"%s\",\"error\":%d,\"ms\":%lld}\n", id, stage, error, at < 0 ? elapsed() : at);
}
struct Owned {
  unsigned id = 0;
  SOCKET socket = INVALID_SOCKET;
  WSAEVENT event = WSA_INVALID_EVENT;
  bool pending = false, ready = false, observed = false, connected = false, closeAttempted = false, enumerationFailed = false;
  long long connectMs = -1;
};
bool api(unsigned id, const char* stage, int result) {
  const int error = result == SOCKET_ERROR ? WSAGetLastError() : 0;
  record(id, stage, result, error);
  if (result == SOCKET_ERROR) { fail(id, stage, error); return false; }
  return true;
}
void outcome(Owned& item, const char* stage, int result, int error, long long at = -1) {
  record(item.id, stage, result, error, at);
  if (result == 0) {
    item.connected = true;
    if (item.id % 2 == 1) ++successes;
  } else if (error == WSAECONNREFUSED && item.id % 2 == 0) {
    ++refusals;
  } else if (!(std::strcmp(stage, "connect") == 0 && error == WSAEWOULDBLOCK)) {
    fail(item.id, stage, error);
  }
}
bool admit(unsigned id, long long groupStart, long long now = elapsed()) {
  if (now >= kWorkMs) { fail(0, "work-deadline", WSAETIMEDOUT); return false; }
  if (now - groupStart >= kGroupMs) { fail((id / 12) * 12, "group-deadline", WSAETIMEDOUT); return false; }
  return true;
}
void begin(Owned& item, long long groupStart) {
  if (!admit(item.id, groupStart)) return;
  item.socket = socket(item.id % 2 == 0 ? AF_INET6 : AF_INET, SOCK_STREAM, IPPROTO_TCP);
  const int socketError = item.socket == INVALID_SOCKET ? WSAGetLastError() : 0;
  record(item.id, "socket", item.socket == INVALID_SOCKET ? -1 : 0, socketError);
  if (item.socket == INVALID_SOCKET) { fail(item.id, "socket", socketError); return; }
  ++opened;
  if (opened - closed > maxLive) maxLive = opened - closed;
  item.event = WSACreateEvent();
  const int eventError = item.event == WSA_INVALID_EVENT ? WSAGetLastError() : 0;
  record(item.id, "event-create", item.event == WSA_INVALID_EVENT ? -1 : 0, eventError);
  if (item.event == WSA_INVALID_EVENT) { fail(item.id, "event-create", eventError); return; }
  ++eventsOpened;
  if (!api(item.id, "event-select", WSAEventSelect(item.socket, item.event, FD_CONNECT))) return;
  BOOL value = randomize ? TRUE : FALSE;
  if (!api(item.id, "set-randomize", setsockopt(item.socket, SOL_SOCKET, SO_RANDOMIZE_PORT, reinterpret_cast<const char*>(&value), sizeof(value)))) return;
  BOOL actual = FALSE;
  int length = sizeof(actual);
  int result = getsockopt(item.socket, SOL_SOCKET, SO_RANDOMIZE_PORT, reinterpret_cast<char*>(&actual), &length);
  int error = result == SOCKET_ERROR ? WSAGetLastError() : 0;
  if (result != SOCKET_ERROR && (length != static_cast<int>(sizeof(actual)) || !!actual != randomize)) { result = SOCKET_ERROR; error = WSAEINVAL; }
  record(item.id, "get-randomize", result == SOCKET_ERROR ? -1 : (actual ? 1 : 0), error);
  if (result == SOCKET_ERROR) { fail(item.id, "get-randomize", error); return; }
  const auto connectStart = elapsed();
  if (!admit(item.id, groupStart, connectStart)) return;
  item.connectMs = connectStart;
  if (item.id % 2 == 0) {
    sockaddr_in6 address{};
    address.sin6_family = AF_INET6; address.sin6_port = htons(3004); address.sin6_addr = in6addr_loopback;
    result = connect(item.socket, reinterpret_cast<const sockaddr*>(&address), sizeof(address));
  } else {
    sockaddr_in address{};
    address.sin_family = AF_INET; address.sin_port = htons(3004); address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    result = connect(item.socket, reinterpret_cast<const sockaddr*>(&address), sizeof(address));
  }
  error = result == SOCKET_ERROR ? WSAGetLastError() : 0;
  ++starts;
  item.pending = result == SOCKET_ERROR && error == WSAEWOULDBLOCK;
  outcome(item, "connect", result, error, item.connectMs);
}
// Polling does not consume the FD_CONNECT result. Only the consuming lanes call
// WSAEnumNetworkEvents. Deferred lanes inspect readiness immediately before close.
void observe(Owned& item, bool closing) {
  if (!item.pending || item.observed) return;
  const DWORD result = WSAWaitForMultipleEvents(1, &item.event, FALSE, 0, FALSE);
  if (result == WSA_WAIT_TIMEOUT && !closing) return;
  item.observed = true;
  if (result == WSA_WAIT_EVENT_0 || result == WSA_WAIT_TIMEOUT) {
    item.ready = result == WSA_WAIT_EVENT_0;
    record(item.id, "event-ready", static_cast<int>(result), 0);
  } else {
    const int error = result == WSA_WAIT_FAILED ? WSAGetLastError() : WSAEINVAL;
    record(item.id, "event-ready", -1, error); fail(item.id, "event-ready", error);
  }
}
void consume(Owned& item) {
  if (!item.pending || item.enumerationFailed) return;
  observe(item, false);
  if (!item.ready) return;
  WSANETWORKEVENTS events{};
  const int result = WSAEnumNetworkEvents(item.socket, item.event, &events);
  int error = result == SOCKET_ERROR ? WSAGetLastError() : 0;
  record(item.id, "completion-api", result, error);
  if (result == SOCKET_ERROR) { item.enumerationFailed = true; fail(item.id, "completion-api", error); return; }
  error = (events.lNetworkEvents & FD_CONNECT) ? events.iErrorCode[FD_CONNECT_BIT] : WSAEINVAL;
  item.pending = false;
  outcome(item, "completion-consumed", error ? -1 : 0, error);
}
void release(Owned& item) {
  if (item.socket == INVALID_SOCKET || item.closeAttempted) return;
  // Never claim that a would-block connect is still kernel-pending. A ready
  // event can contain a refusal that this lane deliberately has not consumed.
  observe(item, true);
  if (item.ready && (item.id % 2 == 1 || (item.id % 12) / 2 < 3)) consume(item);
  record(item.id, "close-intent", item.pending ? 1 : 0, 0);
  if (item.pending) { ++unhandled; if (item.ready) ++readyAtClose; }
  item.closeAttempted = true;
  if (!api(item.id, "closesocket", closesocket(item.socket))) return;
  item.socket = INVALID_SOCKET; ++closed;
  if (item.event != WSA_INVALID_EVENT) {
    const BOOL result = WSACloseEvent(item.event);
    const int error = result ? 0 : WSAGetLastError();
    record(item.id, "event-close", result ? 0 : -1, error);
    if (result) { ++eventsClosed; item.event = WSA_INVALID_EVENT; }
    else fail(item.id, "event-close", error);
  }
}
}

int main(int argc, char** argv) {
  if (argc != 2 || (std::strcmp(argv[1], "plain") && std::strcmp(argv[1], "randomized"))) return 2;
  randomize = !std::strcmp(argv[1], "randomized"); started = Clock::now();
  std::printf("{\"type\":\"header\",\"schema\":1,\"profile\":\"dual-stack-connect\",\"mode\":\"%s\",\"groups\":40,\"lanes\":6,\"fallback_ms\":300,\"group_ms\":700,\"work_ms\":30000,\"cleanup_ms\":35000,\"max_live\":12,\"max_starts\":480}\n", argv[1]);
  WSADATA data{};
  const int startup = WSAStartup(MAKEWORD(2, 2), &data);
  if (startup) fail(0, "startup", startup);
  for (unsigned group = 0; group < kGroups && !failures; ++group) {
    if (elapsed() >= kWorkMs) { fail(0, "work-deadline", WSAETIMEDOUT); break; }
    std::array<Owned, kLanes * 2> sockets{};
    for (unsigned index = 0; index < sockets.size(); ++index) sockets[index].id = group * 12 + index;
    const auto groupStart = elapsed();
    std::printf("{\"type\":\"group\",\"id\":%u,\"ms\":%lld}\n", group, groupStart);
    for (unsigned lane = 0; lane < kLanes && !failures; ++lane) begin(sockets[lane * 2], groupStart);
    while (!failures) {
      if (elapsed() >= kWorkMs) { fail(0, "work-deadline", WSAETIMEDOUT); break; }
      if (elapsed() - groupStart >= kGroupMs) { fail(group * 12, "group-deadline", WSAETIMEDOUT); break; }
      bool done = true;
      for (unsigned lane = 0; lane < kLanes && !failures; ++lane) {
        auto& v6 = sockets[lane * 2]; auto& v4 = sockets[lane * 2 + 1];
        if (lane < 3 && !v6.closeAttempted) consume(v6);
        if (failures) break;
        if (v4.connectMs < 0 && elapsed() - v6.connectMs >= kFallbackMs) begin(v4, groupStart);
        if (failures) break;
        if (!v4.closeAttempted) consume(v4);
        if (failures) break;
        if (v4.connected && !v4.closeAttempted) { release(v6); release(v4); }
        if (!v4.closeAttempted) done = false;
      }
      if (done) break;
      Sleep(1);
    }
    for (auto& item : sockets) release(item);
  }
  // WSACleanup is process-wide; only call after all socket/event close attempts.
  if (!startup && WSACleanup() == SOCKET_ERROR) fail(0, "wsa-cleanup", WSAGetLastError());
  const auto finalElapsed = elapsed();
  if (finalElapsed > kCleanupMs) fail(0, "cleanup-deadline", WSAETIMEDOUT, finalElapsed);
  const bool complete = !failures && starts == 480 && successes == 240 && opened == closed && eventsOpened == eventsClosed;
  std::printf("{\"type\":\"summary\",\"starts\":%u,\"ipv4_successes\":%u,\"expected_refusals\":%u,\"closed_before_handling\":%u,\"ready_before_close\":%u,\"sockets_opened\":%u,\"sockets_closed\":%u,\"events_opened\":%u,\"events_closed\":%u,\"max_live\":%u,\"failures\":%u,\"operation_records\":%u,\"complete\":%s,\"elapsed_ms\":%lld}\n", starts, successes, refusals, unhandled, readyAtClose, opened, closed, eventsOpened, eventsClosed, maxLive, failures, operations, complete ? "true" : "false", finalElapsed);
  return complete && unhandled > 0 ? 0 : 1;
}
