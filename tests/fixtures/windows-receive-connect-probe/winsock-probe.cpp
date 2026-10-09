// Diagnostic only: fixed, owned loopback fixture; never changes OS settings.
#define WIN32_LEAN_AND_MEAN
#include <winsock2.h>
#include <ws2tcpip.h>
#include <mstcpip.h>
#include <windows.h>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <array>
#include <memory>
#include "response-parser.h"
#pragma comment(lib, "Ws2_32.lib")

namespace {
using Clock = std::chrono::steady_clock;
constexpr int kExchanges = 6, kCleanupMs = 35000;
constexpr int kWidth = 6, kPools = 20, kIntervalMs = 100;
constexpr int kOperationMs = 1000, kTotalMs = 30000;
constexpr unsigned kMaxOperationRecords = 30000;
constexpr size_t kMaxResponse = 8192;
Clock::time_point started;
unsigned opened = 0, closed = 0, eventsOpened = 0, eventsClosed = 0;
unsigned failures = 0, attempts = 0, successes = 0, operationRecords = 0;
unsigned syncWrites = 0, asyncWrites = 0, pendingAtClose = 0, postCloseSignals = 0;
unsigned retainedWrites = 0, bytesSent = 0, bytesReceived = 0;
bool randomize = false, exhausted = false, overlap = false;
unsigned maxLive = 0; int firstError = 0;
std::array<int, 121> exchanges{};
long long bootMs() { return static_cast<long long>(GetTickCount64()); }
long long elapsed() { return std::chrono::duration_cast<std::chrono::milliseconds>(Clock::now() - started).count(); }
void record(unsigned id, const char* stage, int result, int error) {
  if (operationRecords >= kMaxOperationRecords) {
    if (!exhausted) {
      if (!failures) firstError = WSAEMSGSIZE;
      ++failures;
      std::printf("{\"type\":\"failure\",\"id\":%u,\"stage\":\"output-cap\",\"error\":10040,\"ms\":%lld}\n", id, elapsed());
    }
    exhausted = true; return;
  }
  ++operationRecords;
  std::printf("{\"type\":\"operation\",\"id\":%u,\"pool\":%u,\"lane\":%u,\"exchange\":%d,\"stage\":\"%s\",\"result\":%d,\"error\":%d,\"ms\":%lld,\"boot_ms\":%lld}\n", id, id / 6, id % 6, id < exchanges.size() ? exchanges[id] : -1, stage, result, error, elapsed(), bootMs());
}
void fail(unsigned id, const char* stage, int error) {
  if (!failures) firstError = error;
  ++failures; exhausted = true;
  std::printf("{\"type\":\"failure\",\"id\":%u,\"stage\":\"%s\",\"error\":%d,\"ms\":%lld}\n", id, stage, error, elapsed());
}
struct WriteState {
  WSAOVERLAPPED overlapped{};
  std::array<char, 256> buffer{};
  WSABUF descriptor{};
  bool pending = false, asynchronous = false;
  size_t submitted = 0;
};
struct Owned {
  unsigned id;
  SOCKET socket = INVALID_SOCKET;
  WSAEVENT event = WSA_INVALID_EVENT;
  WSAEVENT writeEvent = WSA_INVALID_EVENT;
  bool connected = false, closeAttempted = false, released = false, readsInitialized = false;
  std::unique_ptr<WriteState> write = std::make_unique<WriteState>();
  Clock::time_point connectDeadline{};
  enum class Phase { connecting, idle, writing, reading, done } phase = Phase::connecting;
  std::string request, response;
  size_t sent = 0, wouldBlockAt = kMaxResponse + 1;
  bool streaming = false, prefix = false;
  Clock::time_point deadline{};
  explicit Owned(unsigned value): id(value) {}
  Owned(const Owned&) = delete;
  Owned& operator=(const Owned&) = delete;
  void closeSocket() {
    if (closeAttempted) return;
    closeAttempted = true;
    record(id, "pending-at-close", write->pending ? 1 : 0, 0);
    if (write->pending) { ++pendingAtClose; fail(id, "pending-at-close", WSA_IO_INCOMPLETE); }
    if (socket != INVALID_SOCKET) {
      int result = shutdown(socket, SD_SEND);
      int error = result == SOCKET_ERROR ? WSAGetLastError() : 0;
      record(id, "shutdown", result, error);
      // WSAENOTCONN is normal after a rejected connect; retain it, do not mask it.
      if (result == SOCKET_ERROR && connected && error != WSAENOTCONN) fail(id, "shutdown", error);
      result = closesocket(socket);
      error = result == SOCKET_ERROR ? WSAGetLastError() : 0;
      record(id, "closesocket", result, error);
      if (result == 0) { ++closed; socket = INVALID_SOCKET; }
      else { fail(id, "closesocket", error); exhausted = true; }
    }
  }
  void releaseResources() {
    if (released) return;
    released = true;
    if (write->pending) {
      // Only the retained event is used after close, never a recycled socket handle.
      auto remaining = kCleanupMs - elapsed();
      DWORD wait = remaining > 0 ? static_cast<DWORD>(remaining) : 0;
      DWORD result = WSAWaitForMultipleEvents(1, &writeEvent, FALSE, wait, FALSE);
      int error = result == WSA_WAIT_FAILED ? WSAGetLastError() : 0;
      record(id, "post-close-wait", static_cast<int>(result), error);
      if (result == WSA_WAIT_EVENT_0) {
        ++postCloseSignals;
        record(id, "post-close-signal", 0, 0);
      }
      // Notification alone does not record terminal status or bytes. Fail closed
      // and retain every operation that crossed close, even if signaled.
      ++retainedWrites;
      record(id, "write-retained", 1, 0);
      fail(id, "completion-incomplete", WSA_IO_INCOMPLETE);
      exhausted = true;
      (void)write.release(); // Keep OVERLAPPED and buffer alive until process exit.
      writeEvent = WSA_INVALID_EVENT; // Retain the associated event as well.
    }
    if (event != WSA_INVALID_EVENT) {
      BOOL result = WSACloseEvent(event);
      int error = result ? 0 : WSAGetLastError();
      record(id, "event-close", result ? 0 : -1, error);
      if (result) ++eventsClosed; else { fail(id, "event-close", error); exhausted = true; }
    }
    if (writeEvent != WSA_INVALID_EVENT) {
      BOOL result = WSACloseEvent(writeEvent);
      int error = result ? 0 : WSAGetLastError();
      record(id, "write-event-close", result ? 0 : -1, error);
      if (result) ++eventsClosed; else { fail(id, "write-event-close", error); exhausted = true; }
    }
  }
  ~Owned() { closeSocket(); releaseResources(); }
};
bool withinBudget(unsigned id) {
  if (exhausted) return false;
  if (elapsed() < kTotalMs) return true;
  if (!exhausted) fail(id, "total-deadline", WSAETIMEDOUT);
  exhausted = true;
  return false;
}
void bindingState(Owned& item, const char* stage) {
  sockaddr_in local{}; int length = sizeof(local);
  int result = getsockname(item.socket, reinterpret_cast<sockaddr*>(&local), &length);
  int error = result == SOCKET_ERROR ? WSAGetLastError() : 0;
  // Export only whether a local port is assigned, never the address or port.
  record(item.id, stage, result == SOCKET_ERROR ? -1 : local.sin_port ? 1 : 0, error);
}
bool connectStart(Owned& item) {
  if (!withinBudget(item.id)) return false;
  item.socket = WSASocketW(AF_INET, SOCK_STREAM, IPPROTO_TCP, nullptr, 0, WSA_FLAG_OVERLAPPED);
  int error = item.socket == INVALID_SOCKET ? WSAGetLastError() : 0;
  record(item.id, "socket", item.socket == INVALID_SOCKET ? -1 : 0, error);
  if (item.socket == INVALID_SOCKET) { fail(item.id, "socket", error); return false; }
  ++opened;
  if (opened - closed > maxLive) maxLive = opened - closed;
  u_long nonblocking = 1;
  int result = ioctlsocket(item.socket, FIONBIO, &nonblocking);
  error = result == SOCKET_ERROR ? WSAGetLastError() : 0;
  record(item.id, "nonblocking", result, error);
  if (result == SOCKET_ERROR) { fail(item.id, "nonblocking", error); return false; }
  BOOL noDelay = TRUE;
  result = setsockopt(item.socket, IPPROTO_TCP, TCP_NODELAY, reinterpret_cast<const char*>(&noDelay), sizeof(noDelay));
  error = result == SOCKET_ERROR ? WSAGetLastError() : 0;
  record(item.id, "nodelay", result, error);
  if (result == SOCKET_ERROR) { fail(item.id, "nodelay", error); return false; }
  tcp_keepalive keepalive{1, 45000, 45000}; DWORD returned = 0;
  result = WSAIoctl(item.socket, SIO_KEEPALIVE_VALS, &keepalive, sizeof(keepalive), nullptr, 0, &returned, nullptr, nullptr);
  error = result == SOCKET_ERROR ? WSAGetLastError() : 0;
  record(item.id, "keepalive", result, error);
  if (result == SOCKET_ERROR) { fail(item.id, "keepalive", error); return false; }
  item.event = WSACreateEvent();
  error = item.event == WSA_INVALID_EVENT ? WSAGetLastError() : 0;
  record(item.id, "event-create", item.event == WSA_INVALID_EVENT ? -1 : 0, error);
  if (item.event == WSA_INVALID_EVENT) { fail(item.id, "event-create", error); return false; }
  ++eventsOpened;
  // Default Chromium CoreImpl also creates a separate overlapped-write event.
  item.writeEvent = WSACreateEvent();
  error = item.writeEvent == WSA_INVALID_EVENT ? WSAGetLastError() : 0;
  record(item.id, "write-event-create", item.writeEvent == WSA_INVALID_EVENT ? -1 : 0, error);
  if (item.writeEvent == WSA_INVALID_EVENT) { fail(item.id, "write-event-create", error); return false; }
  ++eventsOpened;
  result = WSAEventSelect(item.socket, item.event, FD_CONNECT);
  error = result == SOCKET_ERROR ? WSAGetLastError() : 0;
  record(item.id, "event-select-connect", result, error);
  if (result == SOCKET_ERROR) { fail(item.id, "event-select-connect", error); return false; }
  BOOL value = randomize ? TRUE : FALSE;
  result = setsockopt(item.socket, SOL_SOCKET, SO_RANDOMIZE_PORT, reinterpret_cast<const char*>(&value), sizeof(value));
  error = result == SOCKET_ERROR ? WSAGetLastError() : 0;
  record(item.id, "set-randomize", result, error);
  if (result == SOCKET_ERROR) { fail(item.id, "set-randomize", error); return false; }
  BOOL actual = FALSE; int length = sizeof(actual);
  result = getsockopt(item.socket, SOL_SOCKET, SO_RANDOMIZE_PORT, reinterpret_cast<char*>(&actual), &length);
  error = result == SOCKET_ERROR ? WSAGetLastError() : 0;
  record(item.id, "get-randomize", result, error);
  if (!exhausted) {
    // Only the bounded Boolean and returned byte length leave this process.
    if (result == SOCKET_ERROR) {
      std::printf("{\"type\":\"option\",\"id\":%u,\"value\":null,\"length\":null,\"ms\":%lld}\n", item.id, elapsed());
    } else {
      std::printf("{\"type\":\"option\",\"id\":%u,\"value\":%s,\"length\":%d,\"ms\":%lld}\n", item.id, actual ? "true" : "false", length >= 0 && length <= 65535 ? length : -1, elapsed());
    }
  }
  if (result == SOCKET_ERROR || length != static_cast<int>(sizeof(actual)) || !!actual != randomize) {
    fail(item.id, "verify-randomize", error ? error : WSAEINVAL); return false;
  }
  sockaddr_in address{}; address.sin_family = AF_INET; address.sin_port = htons(3000);
  address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
  // No bind: Chromium's client connect path requests implicit local binding.
  bindingState(item, "binding-before");
  item.connectDeadline = Clock::now() + std::chrono::milliseconds(kOperationMs);
  if (!withinBudget(item.id)) return false;
  ++attempts;
  record(item.id, "connect-submit", 0, 0);
  result = connect(item.socket, reinterpret_cast<sockaddr*>(&address), sizeof(address));
  error = result == SOCKET_ERROR ? WSAGetLastError() : 0;
  record(item.id, "connect", result, error);
  bindingState(item, "binding-after");
  if (result == 0) {
    if (Clock::now() >= item.connectDeadline) { fail(item.id, "connect-deadline", WSAETIMEDOUT); return false; }
    if (!withinBudget(item.id)) return false;
    item.connected = true; item.phase = Owned::Phase::idle; return true;
  }
  if (error != WSAEWOULDBLOCK) { fail(item.id, "connect-sync", error); return false; }
  return true;
}
// No outstanding WSARecv is created. Readiness and consumed bytes below are
// application observations, not evidence that a kernel receive is pending.
bool selectReads(Owned& item) {
  if (item.readsInitialized) return true;
  int result = WSAEventSelect(item.socket, item.event, FD_READ | FD_CLOSE);
  int error = result == SOCKET_ERROR ? WSAGetLastError() : 0;
  record(item.id, "event-select-read-close", result, error);
  if (result == SOCKET_ERROR) { fail(item.id, "event-select-read-close", error); return false; }
  item.readsInitialized = true; return true;
}
bool pollConnect(Owned& item) {
  if (item.connected) return true;
  if (Clock::now() >= item.connectDeadline) { fail(item.id, "connect-deadline", WSAETIMEDOUT); return false; }
  DWORD waited = WSAWaitForMultipleEvents(1, &item.event, FALSE, 0, FALSE);
  if (waited == WSA_WAIT_TIMEOUT) return true;
  int error = waited == WSA_WAIT_FAILED ? WSAGetLastError() : 0;
  record(item.id, "connect-wait", static_cast<int>(waited), error);
  if (waited != WSA_WAIT_EVENT_0) { fail(item.id, "connect-wait", error ? error : WSAEINVAL); return false; }
  WSANETWORKEVENTS events{};
  int result = WSAEnumNetworkEvents(item.socket, item.event, &events);
  error = result == SOCKET_ERROR ? WSAGetLastError() : 0;
  record(item.id, "connect-enumerate", result, error);
  if (result == SOCKET_ERROR) { fail(item.id, "connect-enumerate", error); return false; }
  if (!(events.lNetworkEvents & FD_CONNECT)) { fail(item.id, "connect-event-missing", WSAEINVAL); return false; }
  error = events.iErrorCode[FD_CONNECT_BIT];
  record(item.id, "connect-async", error ? -1 : 0, error);
  if (error) { fail(item.id, "connect-async", error); return false; }
  if (Clock::now() >= item.connectDeadline || !withinBudget(item.id)) { fail(item.id, "connect-deadline", WSAETIMEDOUT); return false; }
  item.connected = true; item.phase = Owned::Phase::idle; return true;
}
bool beginHttp(Owned& item, int exchange, bool streaming) {
  if (!withinBudget(item.id)) return false;
  if (!item.connected || item.write->pending ||
      (item.phase != Owned::Phase::idle && item.phase != Owned::Phase::done)) {
    fail(item.id, "exchange-ownership", WSAEINVAL); return false;
  }
  exchanges[item.id] = exchange;
  item.streaming = streaming; item.prefix = false; item.sent = 0; item.wouldBlockAt = kMaxResponse + 1; item.response.clear();
  const std::string path = streaming ? "/__transport_overlap?pool=" + std::to_string(item.id / kWidth) : "/readyz";
  item.request = "GET " + path + " HTTP/1.1\r\nHost: 127.0.0.1:3000\r\nConnection: keep-alive\r\n\r\n";
  if (item.request.size() > item.write->buffer.size()) { fail(item.id, "request-cap", WSAEMSGSIZE); return false; }
  item.deadline = Clock::now() + std::chrono::milliseconds(kOperationMs);
  record(item.id, "exchange-begin", exchange, 0);
  if (!selectReads(item)) return false;
  item.phase = Owned::Phase::writing; return true;
}
bool completeWrite(Owned& item) {
  auto& write = *item.write;
  DWORD transferred = 0, flags = 0;
  BOOL completed = WSAGetOverlappedResult(item.socket, &write.overlapped, &transferred, FALSE, &flags);
  int error = completed ? 0 : WSAGetLastError();
  // Even synchronous WSASend can return an anomalous INCOMPLETE here. In that
  // case storage remains owned until bounded cleanup/process exit.
  write.pending = !completed && error == WSA_IO_INCOMPLETE;
  record(item.id, write.asynchronous ? "write-complete-async" : "write-complete-sync", completed ? static_cast<int>(transferred) : -1, error);
  if (!completed) { fail(item.id, "write-complete", error); return false; }
  if (write.asynchronous) ++asyncWrites; else ++syncWrites;
  bytesSent += transferred;
  if (!transferred || transferred > write.submitted) { fail(item.id, "write-length", WSAEINVAL); return false; }
  item.sent += transferred;
  if (item.sent == item.request.size()) item.phase = Owned::Phase::reading;
  return true;
}
bool progressWrite(Owned& item) {
  auto& write = *item.write;
  if (write.pending) {
    DWORD waited = WSAWaitForMultipleEvents(1, &item.writeEvent, FALSE, 0, FALSE);
    if (waited == WSA_WAIT_TIMEOUT) return true;
    int error = waited == WSA_WAIT_FAILED ? WSAGetLastError() : 0;
    record(item.id, "write-wait", static_cast<int>(waited), error);
    if (waited != WSA_WAIT_EVENT_0) { fail(item.id, "write-wait", error ? error : WSAEINVAL); return false; }
    return completeWrite(item);
  }
  BOOL reset = WSAResetEvent(item.writeEvent);
  int error = reset ? 0 : WSAGetLastError();
  record(item.id, "write-reset", reset ? 0 : -1, error);
  if (!reset) { fail(item.id, "write-reset", error); return false; }
  write.overlapped = {}; write.overlapped.hEvent = item.writeEvent;
  write.submitted = item.request.size() - item.sent;
  std::memcpy(write.buffer.data(), item.request.data() + item.sent, write.submitted);
  write.descriptor = { static_cast<ULONG>(write.submitted), write.buffer.data() };
  record(item.id, "write-submit-bytes", static_cast<int>(write.submitted), 0);
  int result = WSASend(item.socket, &write.descriptor, 1, nullptr, 0, &write.overlapped, nullptr);
  error = result == SOCKET_ERROR ? WSAGetLastError() : 0;
  record(item.id, "write-submit", result, error);
  if (result == SOCKET_ERROR && error != WSA_IO_PENDING) { fail(item.id, "write-submit", error); return false; }
  write.asynchronous = result == SOCKET_ERROR; write.pending = write.asynchronous;
  if (write.pending) return true;
  return completeWrite(item);
}
bool progressRead(Owned& item) {
  // Polling with zero timeout never holds up sibling connect/write machines.
  DWORD waited = WSAWaitForMultipleEvents(1, &item.event, FALSE, 0, FALSE);
  if (waited != WSA_WAIT_TIMEOUT) {
    int error = waited == WSA_WAIT_FAILED ? WSAGetLastError() : 0;
    record(item.id, "read-wait", static_cast<int>(waited), error);
    if (waited != WSA_WAIT_EVENT_0) { fail(item.id, "read-wait", error ? error : WSAEINVAL); return false; }
    WSANETWORKEVENTS events{};
    int result = WSAEnumNetworkEvents(item.socket, item.event, &events);
    error = result == SOCKET_ERROR ? WSAGetLastError() : 0;
    record(item.id, "read-enumerate", result, error);
    if (result == SOCKET_ERROR) { fail(item.id, "read-enumerate", error); return false; }
    record(item.id, "read-events", static_cast<int>(events.lNetworkEvents), 0);
    if (events.lNetworkEvents & ~(FD_READ | FD_CLOSE)) { fail(item.id, "read-event-unexpected", WSAEINVAL); return false; }
    if (events.lNetworkEvents & FD_READ) {
      error = events.iErrorCode[FD_READ_BIT];
      record(item.id, "read-event-error", error ? -1 : 0, error);
      if (error) { fail(item.id, "read-event-error", error); return false; }
    }
    if (events.lNetworkEvents & FD_CLOSE) {
      error = events.iErrorCode[FD_CLOSE_BIT];
      record(item.id, "close-event-error", error ? -1 : 0, error);
      fail(item.id, "unexpected-peer-close", error ? error : WSAECONNRESET); return false;
    }
  }
  char buffer[1024];
  size_t capacity = kMaxResponse - item.response.size();
  if (!capacity) { fail(item.id, "response-cap", WSAEMSGSIZE); return false; }
  if (capacity > sizeof(buffer)) capacity = sizeof(buffer);
  int result = recv(item.socket, buffer, static_cast<int>(capacity), 0);
  int error = result == SOCKET_ERROR ? WSAGetLastError() : 0;
  // Record would-block once per consumed length, not every 1ms poll.
  if (result == SOCKET_ERROR && error == WSAEWOULDBLOCK) {
    if (item.wouldBlockAt != item.response.size()) {
      record(item.id, "http-recv", result, error); item.wouldBlockAt = item.response.size();
    }
    return true;
  }
  record(item.id, "http-recv", result, error);
  if (result <= 0) { fail(item.id, "http-recv", error ? error : WSAECONNRESET); return false; }
  bytesReceived += static_cast<unsigned>(result);
  item.response.append(buffer, static_cast<size_t>(result));
  auto parsed = owned_probe::parse_response(item.response, item.streaming);
  const size_t consumed = owned_probe::body_bytes(item.response);
  record(item.id, "body-consumed", static_cast<int>(consumed), 0);
  if (parsed == owned_probe::Response::invalid || parsed == owned_probe::Response::oversized) {
    fail(item.id, "fixture-response", WSAEINVAL); return false;
  }
  if (item.streaming && !item.prefix && consumed == 1024 && parsed == owned_probe::Response::pending) {
    item.prefix = true; record(item.id, "prefix-observed", static_cast<int>(consumed), 0);
  }
  if (parsed == owned_probe::Response::complete) {
    if (Clock::now() >= item.deadline || !withinBudget(item.id)) { fail(item.id, "http-deadline", WSAETIMEDOUT); return false; }
    ++successes; record(item.id, "http-complete", exchanges[item.id], 0);
    item.phase = Owned::Phase::done;
  }
  return true;
}
bool progress(Owned& item) {
  if (!withinBudget(item.id)) return false;
  if (item.phase == Owned::Phase::connecting) return pollConnect(item);
  if (item.phase == Owned::Phase::idle || item.phase == Owned::Phase::done) return true;
  if (Clock::now() >= item.deadline) { fail(item.id, "http-deadline", WSAETIMEDOUT); return false; }
  return item.phase == Owned::Phase::writing ? progressWrite(item) : progressRead(item);
}
bool awaitConnected(Owned& item) {
  while (!item.connected) { if (!progress(item)) return false; if (!item.connected) Sleep(1); }
  return true;
}
bool awaitHttp(Owned& item) {
  while (item.phase != Owned::Phase::done) { if (!progress(item)) return false; if (item.phase != Owned::Phase::done) Sleep(1); }
  return true;
}
using Pool = std::array<std::unique_ptr<Owned>, kWidth>;
bool overlapInitial(Pool& active, unsigned pool) {
  active[0] = std::make_unique<Owned>(pool * kWidth);
  auto& first = *active[0];
  if (!connectStart(first) || !awaitConnected(first) || !beginHttp(first, 0, true)) return false;
  // A missed exposure is inconclusive, not a fabricated Winsock error.
  while (!first.prefix) {
    if (!progress(first)) return false;
    if (first.phase == Owned::Phase::done) { fail(first.id, "prefix-not-observed", 0); return false; }
    if (!first.prefix) Sleep(1);
  }
  for (unsigned lane = 1; lane < kWidth; ++lane) {
    if (!withinBudget(first.id)) return false;
    if (first.phase == Owned::Phase::done) { fail(first.id, "premature-final-consumption", 0); return false; }
    active[lane] = std::make_unique<Owned>(pool * kWidth + lane);
    if (!connectStart(*active[lane])) return false;
    // Service lane0 between submissions as well as while connects are pending.
    // If scheduling delays consume the final body early, fail qualification.
    record(first.id, "receive-service", static_cast<int>(lane), 0);
    if (!progress(first)) return false;
  }
  record(first.id, "siblings-submitted", 5, 0);
  while (true) {
    bool done = true;
    for (auto& item : active) {
      if (!progress(*item)) return false;
      if (item->phase == Owned::Phase::idle && !beginHttp(*item, 0, false)) return false;
      if (item->phase != Owned::Phase::done) done = false;
    }
    if (done) return true;
    Sleep(1);
  }
}
bool serializedInitial(Pool& active, unsigned pool) {
  for (unsigned lane = 0; lane < kWidth; ++lane) {
    if (!withinBudget(pool * kWidth + lane)) return false;
    active[lane] = std::make_unique<Owned>(pool * kWidth + lane);
    if (!connectStart(*active[lane])) return false;
  }
  for (auto& item : active) if (!awaitConnected(*item)) return false;
  for (unsigned lane = 0; lane < kWidth; ++lane)
    if (!beginHttp(*active[lane], 0, lane == 0) || !awaitHttp(*active[lane])) return false;
  return true;
}
bool rounds(Pool& active) {
  for (int exchange = 1; exchange < kExchanges; ++exchange) {
    if (!overlap) {
      for (auto& item : active)
        if (!beginHttp(*item, exchange, false) || !awaitHttp(*item)) return false;
    } else {
      for (auto& item : active) if (!beginHttp(*item, exchange, false)) return false;
      while (true) {
        bool done = true;
        for (auto& item : active) {
          if (!progress(*item)) return false;
          if (item->phase != Owned::Phase::done) done = false;
        }
        if (done) break;
        Sleep(1);
      }
    }
  }
  return true;
}
Clock::time_point retire(Pool& active, unsigned pool, bool successful) {
  // Every socket is closed before waiting for cancellation completions. A
  // pending write retains buffer/OVERLAPPED/event until process exit on failure.
  for (auto& item : active) if (item) item->closeSocket();
  const auto lastClose = Clock::now();
  for (auto& item : active) if (item) item->releaseResources();
  for (auto& item : active) item.reset();
  if (successful && !failures) record(pool * kWidth, "pool-retired", static_cast<int>(pool), 0);
  return lastClose;
}
}
int run(int argc, char** argv) {
  if (argc != 3 || (std::strcmp(argv[1], "serialized") && std::strcmp(argv[1], "overlap")) ||
      (std::strcmp(argv[2], "false") && std::strcmp(argv[2], "true"))) return 2;
  overlap = !std::strcmp(argv[1], "overlap"); randomize = !std::strcmp(argv[2], "true");
  exchanges.fill(-1); started = Clock::now();
  WSADATA data{}; int startup = WSAStartup(MAKEWORD(2, 2), &data);
  if (startup) return 3;
  struct WinsockScope { bool active = true; ~WinsockScope() { if (active) WSACleanup(); } } winsock;
  std::printf("{\"type\":\"header\",\"schema\":1,\"profile\":\"receive-connect-history\",\"schedule\":\"%s\",\"randomize\":%s,\"pools\":20,\"width\":6,\"exchanges\":6,\"interval_ms\":100,\"total_cap_ms\":30000,\"cleanup_cap_ms\":35000,\"response_cap\":8192,\"operation_cap\":30000,\"start_boot_ms\":%lld}\n", argv[1], argv[2], bootMs());
  Clock::time_point retiredAt{};
  unsigned completedPools = 0;
  for (unsigned pool = 0; pool <= kPools && withinBudget(pool * kWidth); ++pool) {
    if (pool > 0) {
      const auto nextStart = retiredAt + std::chrono::milliseconds(kIntervalMs);
      while (Clock::now() < nextStart && withinBudget(pool * kWidth)) Sleep(1);
      if (!withinBudget(pool * kWidth)) break;
      record(pool * kWidth, "next-pool-start", static_cast<int>(std::chrono::duration_cast<std::chrono::milliseconds>(Clock::now() - retiredAt).count()), 0);
    }
    Pool active;
    bool successful;
    if (pool == kPools) {
      active[0] = std::make_unique<Owned>(120);
      successful = connectStart(*active[0]) && awaitConnected(*active[0]) && beginHttp(*active[0], 0, false) && awaitHttp(*active[0]);
    } else {
      successful = (overlap ? overlapInitial(active, pool) : serializedInitial(active, pool)) && rounds(active);
    }
    retiredAt = retire(active, pool, successful);
    if (!successful || failures) break;
    if (pool < kPools) ++completedPools;
  }
  int cleanup = WSACleanup(); int error = cleanup == SOCKET_ERROR ? WSAGetLastError() : 0;
  winsock.active = false;
  record(120, "wsa-cleanup", cleanup, error);
  if (cleanup == SOCKET_ERROR) fail(120, "wsa-cleanup", error);
  if (opened != closed || eventsOpened != eventsClosed) fail(120, "ownership", WSAEINVAL);
  if (elapsed() > kTotalMs && !exhausted) withinBudget(120);
  bool complete = attempts == 121 && successes == 721 && completedPools == 20 && !failures && !exhausted && elapsed() <= kTotalMs && retainedWrites == 0;
  std::printf("{\"type\":\"summary\",\"attempts\":%u,\"successes\":%u,\"completed_pools\":%u,\"failures\":%u,\"first_error\":%d,\"sockets_opened\":%u,\"sockets_closed\":%u,\"events_opened\":%u,\"events_closed\":%u,\"max_live\":%u,\"operation_records\":%u,\"complete\":%s,\"elapsed_ms\":%lld,\"sync_writes\":%u,\"async_writes\":%u,\"pending_at_close\":%u,\"post_close_signals\":%u,\"retained_writes\":%u,\"bytes_sent\":%u,\"bytes_received\":%u}\n", attempts, successes, completedPools, failures, firstError, opened, closed, eventsOpened, eventsClosed, maxLive, operationRecords, complete ? "true" : "false", elapsed(), syncWrites, asyncWrites, pendingAtClose, postCloseSignals, retainedWrites, bytesSent, bytesReceived);
  return complete ? 0 : 1;
}
int main(int argc, char** argv) {
  try { return run(argc, argv); }
  catch (...) {
    std::fputs("Native probe exception; evidence is incomplete.\n", stderr);
    return 4;
  }
}
