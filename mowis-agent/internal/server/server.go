package server

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"sync"
	"time"

	"github.com/mowisai/mowis-agent/internal/app"
	"github.com/mowisai/mowis-agent/internal/logging"
	"github.com/mowisai/mowis-agent/internal/message"
	"github.com/mowisai/mowis-agent/internal/permission"
	"github.com/mowisai/mowis-agent/internal/session"
	"github.com/mowisai/mowis-agent/internal/version"
)

// Start launches the HTTP API server and blocks until ctx is cancelled.
func Start(ctx context.Context, a *app.App, cwd, hostname string, port int) error {
	mux := http.NewServeMux()
	h := &handler{app: a, cwd: cwd}

	// Health
	mux.HandleFunc("GET /health", h.health)

	// Sessions
	mux.HandleFunc("GET /session", h.listSessions)
	mux.HandleFunc("POST /session", h.createSession)
	mux.HandleFunc("GET /session/{id}", h.getSession)
	mux.HandleFunc("DELETE /session/{id}", h.deleteSession)

	// Messages
	mux.HandleFunc("GET /session/{id}/message", h.listMessages)
	mux.HandleFunc("POST /session/{id}/message", h.sendMessage)
	mux.HandleFunc("POST /session/{id}/message/async", h.sendMessageAsync)

	// Agent control
	mux.HandleFunc("POST /session/{id}/abort", h.abortSession)
	mux.HandleFunc("POST /session/{id}/permission/{pid}", h.handlePermission)

	// Events (SSE)
	mux.HandleFunc("GET /event", h.eventStream)

	// Config
	mux.HandleFunc("GET /config", h.getConfig)
	mux.HandleFunc("GET /provider", h.listProviders)
	mux.HandleFunc("GET /agent", h.listAgents)

	addr := fmt.Sprintf("%s:%d", hostname, port)
	srv := &http.Server{Addr: addr, Handler: corsMiddleware(mux)}

	go func() {
		<-ctx.Done()
		logging.Info("Shutting down HTTP server")
		shutdownCtx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		srv.Shutdown(shutdownCtx)
	}()

	logging.Info("HTTP server listening on %s", addr)
	fmt.Printf("mowis-agent listening on http://%s\n", addr)

	if err := srv.ListenAndServe(); err != nil && err != http.ErrServerClosed {
		return fmt.Errorf("HTTP server error: %w", err)
	}
	return nil
}

func corsMiddleware(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Access-Control-Allow-Origin", "*")
		w.Header().Set("Access-Control-Allow-Methods", "GET, POST, PATCH, DELETE, OPTIONS")
		w.Header().Set("Access-Control-Allow-Headers", "Content-Type, Authorization")
		if r.Method == http.MethodOptions {
			w.WriteHeader(http.StatusNoContent)
			return
		}
		next.ServeHTTP(w, r)
	})
}

// ─────────────────────────────────────────────────────────────────────────────
// SSE event broker
// ─────────────────────────────────────────────────────────────────────────────

var (
	sseClients   = map[chan sseEvent]struct{}{}
	sseClientsMu sync.Mutex
)

type sseEvent struct {
	Type    string      `json:"type"`
	Payload interface{} `json:"payload"`
}

func broadcastSSE(evt sseEvent) {
	sseClientsMu.Lock()
	defer sseClientsMu.Unlock()
	for ch := range sseClients {
		select {
		case ch <- evt:
		default:
		}
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// handler
// ─────────────────────────────────────────────────────────────────────────────

type handler struct {
	app *app.App
	cwd string
}

// ── Health ───────────────────────────────────────────────────────────────────

func (h *handler) health(w http.ResponseWriter, r *http.Request) {
	writeJSON(w, http.StatusOK, map[string]interface{}{
		"healthy": true,
		"version": version.Version,
		"cwd":     h.cwd,
	})
}

// ── Sessions ────────────────────────────────────────────────────────────────

func (h *handler) listSessions(w http.ResponseWriter, r *http.Request) {
	sessions, err := h.app.Sessions.List(r.Context())
	if err != nil {
		writeError(w, http.StatusInternalServerError, err)
		return
	}
	writeJSON(w, http.StatusOK, sessions)
}

func (h *handler) createSession(w http.ResponseWriter, r *http.Request) {
	var req struct {
		Title string `json:"title"`
	}
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		writeError(w, http.StatusBadRequest, err)
		return
	}
	if req.Title == "" {
		req.Title = "New Session"
	}
	sess, err := h.app.Sessions.Create(r.Context(), req.Title)
	if err != nil {
		writeError(w, http.StatusInternalServerError, err)
		return
	}
	broadcastSSE(sseEvent{Type: "session.created", Payload: sess})
	writeJSON(w, http.StatusCreated, sess)
}

func (h *handler) getSession(w http.ResponseWriter, r *http.Request) {
	sess, err := h.app.Sessions.Get(r.Context(), r.PathValue("id"))
	if err != nil {
		writeError(w, http.StatusNotFound, err)
		return
	}
	writeJSON(w, http.StatusOK, sess)
}

func (h *handler) deleteSession(w http.ResponseWriter, r *http.Request) {
	id := r.PathValue("id")
	if err := h.app.Sessions.Delete(r.Context(), id); err != nil {
		writeError(w, http.StatusInternalServerError, err)
		return
	}
	broadcastSSE(sseEvent{Type: "session.deleted", Payload: map[string]string{"id": id}})
	writeJSON(w, http.StatusOK, map[string]string{"status": "deleted"})
}

// ── Messages ────────────────────────────────────────────────────────────────

func (h *handler) listMessages(w http.ResponseWriter, r *http.Request) {
	msgs, err := h.app.Messages.List(r.Context(), r.PathValue("id"))
	if err != nil {
		writeError(w, http.StatusInternalServerError, err)
		return
	}
	writeJSON(w, http.StatusOK, msgs)
}

type messageRequest struct {
	Text string `json:"text"`
}

// sendMessage — blocking: waits for agent to finish.
func (h *handler) sendMessage(w http.ResponseWriter, r *http.Request) {
	sessionID := r.PathValue("id")
	var req messageRequest
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		writeError(w, http.StatusBadRequest, err)
		return
	}
	if req.Text == "" {
		writeError(w, http.StatusBadRequest, fmt.Errorf("text is required"))
		return
	}

	done, err := h.app.CoderAgent.Run(r.Context(), sessionID, req.Text)
	if err != nil {
		writeError(w, http.StatusInternalServerError, err)
		return
	}

	result := <-done
	if result.Error != nil {
		writeError(w, http.StatusInternalServerError, result.Error)
		return
	}

	msgs, err := h.app.Messages.List(r.Context(), sessionID)
	if err != nil {
		writeError(w, http.StatusInternalServerError, err)
		return
	}

	broadcastSSE(sseEvent{Type: "agent.completed", Payload: map[string]interface{}{
		"session_id": sessionID,
	}})

	writeJSON(w, http.StatusOK, map[string]interface{}{
		"session_id": sessionID,
		"messages":   msgs,
	})
}

// sendMessageAsync — non-blocking: returns immediately.
func (h *handler) sendMessageAsync(w http.ResponseWriter, r *http.Request) {
	sessionID := r.PathValue("id")
	var req messageRequest
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		writeError(w, http.StatusBadRequest, err)
		return
	}
	if req.Text == "" {
		writeError(w, http.StatusBadRequest, fmt.Errorf("text is required"))
		return
	}

	// Use background context — r.Context() is cancelled when the handler returns,
	// which would immediately cancel the agent's work.
	done, err := h.app.CoderAgent.Run(context.Background(), sessionID, req.Text)
	if err != nil {
		writeError(w, http.StatusInternalServerError, err)
		return
	}

	go func() {
		result := <-done
		if result.Error != nil {
			broadcastSSE(sseEvent{Type: "agent.error", Payload: map[string]interface{}{
				"session_id": sessionID,
				"error":      result.Error.Error(),
			}})
			return
		}
		broadcastSSE(sseEvent{Type: "agent.completed", Payload: map[string]interface{}{
			"session_id": sessionID,
		}})
	}()

	writeJSON(w, http.StatusAccepted, map[string]string{
		"status":     "accepted",
		"session_id": sessionID,
	})
}

// ── Agent control ────────────────────────────────────────────────────────────

func (h *handler) abortSession(w http.ResponseWriter, r *http.Request) {
	sessionID := r.PathValue("id")
	h.app.CoderAgent.Cancel(sessionID)
	writeJSON(w, http.StatusOK, map[string]string{"status": "aborted"})
}

func (h *handler) handlePermission(w http.ResponseWriter, r *http.Request) {
	pid := r.PathValue("pid")

	var req struct {
		Approve bool `json:"approve"`
		Persist bool `json:"persist"`
	}
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		writeError(w, http.StatusBadRequest, err)
		return
	}

	// Look up the pending permission request by ID
	permReq := permission.PermissionRequest{ID: pid}
	if req.Approve {
		if req.Persist {
			h.app.Permissions.GrantPersistant(permReq)
		} else {
			h.app.Permissions.Grant(permReq)
		}
	} else {
		h.app.Permissions.Deny(permReq)
	}

	writeJSON(w, http.StatusOK, map[string]string{"status": "ok"})
}

// ── SSE event stream ────────────────────────────────────────────────────────

func (h *handler) eventStream(w http.ResponseWriter, r *http.Request) {
	flusher, ok := w.(http.Flusher)
	if !ok {
		http.Error(w, "streaming not supported", http.StatusInternalServerError)
		return
	}

	w.Header().Set("Content-Type", "text/event-stream")
	w.Header().Set("Cache-Control", "no-cache")
	w.Header().Set("Connection", "keep-alive")

	ch := make(chan sseEvent, 64)
	sseClientsMu.Lock()
	sseClients[ch] = struct{}{}
	sseClientsMu.Unlock()

	defer func() {
		sseClientsMu.Lock()
		delete(sseClients, ch)
		sseClientsMu.Unlock()
	}()

	ctx := r.Context()
	unsub := h.subscribeToAppEvents(ch, ctx)
	defer unsub()

	for {
		select {
		case <-ctx.Done():
			return
		case evt, ok := <-ch:
			if !ok {
				return
			}
			data, _ := json.Marshal(evt.Payload)
			fmt.Fprintf(w, "event: %s\ndata: %s\n\n", evt.Type, string(data))
			flusher.Flush()
		}
	}
}

func (h *handler) subscribeToAppEvents(ch chan<- sseEvent, ctx context.Context) func() {
	ctx, cancel := context.WithCancel(ctx)
	var wg sync.WaitGroup

	// Sessions
	wg.Add(1)
	go func() {
		defer wg.Done()
		events := h.app.Sessions.Subscribe(ctx)
		for {
			select {
			case <-ctx.Done():
				return
			case evt, ok := <-events:
				if !ok {
					return
				}
				ch <- sseEvent{Type: "session." + string(evt.Type), Payload: evt.Payload}
			}
		}
	}()

	// Messages
	wg.Add(1)
	go func() {
		defer wg.Done()
		events := h.app.Messages.Subscribe(ctx)
		for {
			select {
			case <-ctx.Done():
				return
			case evt, ok := <-events:
				if !ok {
					return
				}
				ch <- sseEvent{Type: "message." + string(evt.Type), Payload: evt.Payload}
			}
		}
	}()

	// Permissions
	wg.Add(1)
	go func() {
		defer wg.Done()
		events := h.app.Permissions.Subscribe(ctx)
		for {
			select {
			case <-ctx.Done():
				return
			case evt, ok := <-events:
				if !ok {
					return
				}
				ch <- sseEvent{Type: "permission." + string(evt.Type), Payload: evt.Payload}
			}
		}
	}()

	// Agent events
	wg.Add(1)
	go func() {
		defer wg.Done()
		events := h.app.CoderAgent.Subscribe(ctx)
		for {
			select {
			case <-ctx.Done():
				return
			case evt, ok := <-events:
				if !ok {
					return
				}
				ch <- sseEvent{Type: "agent." + string(evt.Type), Payload: evt.Payload}
			}
		}
	}()

	return func() {
		cancel()
		go func() {
			done := make(chan struct{})
			go func() { wg.Wait(); close(done) }()
			select {
			case <-done:
			case <-time.After(3 * time.Second):
			}
		}()
	}
}

// ── Config / Providers / Agents ─────────────────────────────────────────────

func (h *handler) getConfig(w http.ResponseWriter, r *http.Request) {
	writeJSON(w, http.StatusOK, map[string]interface{}{
		"cwd":     h.cwd,
		"version": version.Version,
	})
}

func (h *handler) listProviders(w http.ResponseWriter, r *http.Request) {
	providers := []string{
		"anthropic", "openai", "gemini", "vertexai", "azure",
		"bedrock", "copilot", "groq", "openrouter", "xai",
	}
	writeJSON(w, http.StatusOK, providers)
}

func (h *handler) listAgents(w http.ResponseWriter, r *http.Request) {
	agents := []map[string]string{
		{"id": "coder", "name": "Build Agent", "description": "Full access to all tools."},
		{"id": "coder-plan", "name": "Plan Agent", "description": "Read-only access."},
	}
	writeJSON(w, http.StatusOK, agents)
}

// ── Helpers ──────────────────────────────────────────────────────────────────

func writeJSON(w http.ResponseWriter, status int, v interface{}) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	json.NewEncoder(w).Encode(v)
}

func writeError(w http.ResponseWriter, status int, err error) {
	writeJSON(w, status, map[string]string{"error": err.Error()})
}

// Suppress unused import warnings for types used only in type assertions.
var (
	_ session.Session
	_ message.Message
	_ permission.PermissionRequest
)
