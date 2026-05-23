from textual.app import App, ComposeResult
from textual.screen import Screen
from textual.reactive import reactive
from textual.widget import Widget
from textual.widgets import Static, Input, RichLog
from textual.containers import Vertical, Horizontal, VerticalScroll
from rich.text import Text
from rich.align import Align
from rich.panel import Panel
import os
import asyncio
import random

PURPLE = "#6d28d9"
CYAN = "#22d3ee"
GREEN = "#22c55e"
YELLOW = "#eab308"
RED = "#ef4444"
DIM = "#666666"


# ============================================================
# SPLASH SCREEN
# ============================================================

class SplashArt(Widget):
    frame: reactive[int] = reactive(0)
    show_hint: reactive[bool] = reactive(False)

    def on_mount(self) -> None:
        self.set_interval(0.6, self.tick)

    def tick(self) -> None:
        self.frame += 1
        if self.frame > 4:
            self.show_hint = True

    def render(self) -> Align:
        f = self.frame % 8
        glow_colors = ["#6d28d9", "#7c3aed", "#8b5cf6", "#a855f7", "#8b5cf6", "#7c3aed", "#6d28d9", "#5b21b6"]
        glow = glow_colors[f % len(glow_colors)]

        art = Text()
        art.append("\n\n")
        art.append("       ███╗   ███╗  ██████╗  ██╗    ██╗ ██╗ ███████╗  █████╗  ██╗\n", style=f"bold {glow}")
        art.append("       ████╗ ████║ ██╔═══██╗ ██║    ██║ ██║ ██╔════╝ ██╔══██╗ ██║\n", style=f"bold {glow}")
        art.append("       ██╔████╔██║ ██║   ██║ ██║ █╗ ██║ ██║ ███████╗ ███████║ ██║\n", style=f"bold #8b5cf6")
        art.append("       ██║╚██╔╝██║ ██║   ██║ ██║███╗██║ ██║ ╚════██║ ██╔══██║ ██║\n", style=f"bold #7c3aed")
        art.append("       ██║ ╚═╝ ██║ ╚██████╔╝ ╚███╔███╔╝ ██║ ███████║ ██║  ██║ ██║\n", style=f"bold #6d28d9")
        art.append("       ╚═╝     ╚═╝  ╚═════╝   ╚══╝╚══╝  ╚═╝ ╚══════╝ ╚═╝  ╚═╝ ╚═╝\n", style=f"bold #5b21b6")
        art.append("\n")
        art.append("                            ╔═══════════════════════════════╗\n", style="#4c1d95")
        art.append("                            ║  multi-agent conductor system ║\n", style=f"italic #7c3aed")
        art.append("                            ╚═══════════════════════════════╝\n", style="#4c1d95")
        art.append("\n\n")
        dots_cycle = ["⣾⣽⣻⢿⡿⣟⣯⣷", "⣷⣯⣟⡿⢿⣻⣽⣾", "⣯⣷⣻⣽⣾⣟⡿⢿"]
        dots = dots_cycle[f % len(dots_cycle)]
        art.append(f"                     {dots}  Initializing agents...", style=f"dim {glow}")
        if self.show_hint:
            art.append("\n\n")
            art.append("                          ╭──────────────────────────────╮\n", style="#2a2a4a")
            art.append("                          │", style="#2a2a4a")
            art.append("   Press Enter to start   ", style=f"bold {glow}")
            art.append("│\n", style="#2a2a4a")
            art.append("                          ╰──────────────────────────────╯\n", style="#2a2a4a")
        return Align.center(art, vertical="middle")


class SplashScreen(Screen):
    CSS = """
    SplashScreen {
        background: #08080c;
    }
    SplashArt {
        width: 100%;
        height: 100%;
    }
    """

    def compose(self) -> ComposeResult:
        yield SplashArt()


# ============================================================
# SETUP WIZARD
# ============================================================

PROVIDERS = [
    ("openai", "OpenAI"),
    ("anthropic", "Anthropic"),
    ("google", "Google Gemini"),
    ("mistral", "Mistral"),
    ("groq", "Groq"),
]


class SetupWizard(Screen):
    CSS = """
    SetupWizard {
        background: #08080c;
        color: #e0e0e0;
        align: center middle;
    }
    #wizard-container {
        width: 60;
        height: auto;
        max-height: 80%;
        border: solid #6d28d9;
        padding: 2 4;
        background: #0d0d1a;
    }
    .wizard-title {
        width: 100%;
        text-align: center;
        margin-bottom: 2;
    }
    .step-indicator {
        width: 100%;
        text-align: center;
        margin-bottom: 1;
        color: #6d28d9;
    }
    .wizard-label {
        margin-bottom: 1;
        width: 100%;
    }
    #provider-list {
        height: auto;
        max-height: 12;
        margin-bottom: 2;
        border: solid #2a2a4a;
        background: #08080c;
    }
    .provider-item {
        padding: 0 2;
        height: 1;
    }
    .provider-item.highlighted {
        background: #6d28d9;
        color: #ffffff;
    }
    #api-key-input {
        margin-bottom: 2;
        border: solid #6d28d9;
        background: #08080c;
        color: #ffffff;
    }
    .wizard-hint {
        color: #666666;
        margin-bottom: 1;
        width: 100%;
    }
    """

    BINDINGS = [("escape", "quit", "Quit")]

    def __init__(self, **kwargs):
        super().__init__(**kwargs)
        self.step = 1
        self.selected_provider = 0
        self.provider_id = ""
        self.provider_name = ""
        self.api_key = ""

    def compose(self) -> ComposeResult:
        yield Vertical(
            Static("MowisAI Setup", classes="wizard-title"),
            Static("Step 1 of 2", classes="step-indicator"),
            Static("Select your AI provider:", classes="wizard-label"),
            Vertical(
                *[Static(f"  {name}", classes="provider-item", id=f"provider-{pid}") for pid, name in PROVIDERS],
                id="provider-list"
            ),
            Static("Press Enter to select", classes="wizard-hint"),
            id="wizard-container"
        )

    def on_mount(self) -> None:
        self._highlight_provider()

    def _highlight_provider(self) -> None:
        for i, (pid, name) in enumerate(PROVIDERS):
            try:
                item = self.query_one(f"#provider-{pid}")
                item.set_class(i == self.selected_provider, "highlighted")
            except Exception:
                pass

    def on_key(self, event) -> None:
        if self.step == 1:
            if event.key == "up":
                self.selected_provider = (self.selected_provider - 1) % len(PROVIDERS)
                self._highlight_provider()
                event.prevent_default()
            elif event.key == "down":
                self.selected_provider = (self.selected_provider + 1) % len(PROVIDERS)
                self._highlight_provider()
                event.prevent_default()
            elif event.key == "enter":
                self._go_to_step_2()
                event.prevent_default()

    def _go_to_step_2(self) -> None:
        self.provider_id, self.provider_name = PROVIDERS[self.selected_provider]
        container = self.query_one("#wizard-container")
        container.remove_children()
        container.mount(
            Static("MowisAI Setup", classes="wizard-title"),
            Static("Step 2 of 2", classes="step-indicator"),
            Static(f"Provider: {self.provider_name}", classes="wizard-label"),
            Static("Enter your API key:", classes="wizard-label"),
            ApiKeyInput(placeholder="sk-...", id="api-key-input", password=True),
            Static("Press Enter to continue", classes="wizard-hint"),
        )
        self.query_one("#api-key-input").focus()
        self.step = 2

    def _finish_setup(self) -> None:
        try:
            self.api_key = self.query_one("#api-key-input").value or ""
        except Exception:
            pass
        self.app.switch_screen(MainCLI())

    def action_quit(self) -> None:
        self.app.exit()


class ApiKeyInput(Input):
    def on_input_submitted(self, event) -> None:
        if (event.value or "").strip():
            try:
                self.screen._finish_setup()
            except Exception:
                pass


# ============================================================
# UI COMPONENTS
# ============================================================

class MessageLog(RichLog):
    def add_chat(self, text: str, role: str = "user") -> None:
        if role == "user":
            msg = Text()
            msg.append("• ", style=PURPLE)
            msg.append(text, style="white")
            self.write(msg, width=80)
        elif role == "conductor":
            msg = Text()
            msg.append("◈ ", style=GREEN)
            msg.append(text, style="white")
            self.write(msg, width=80)
        elif role == "system":
            msg = Text(text, style=DIM)
            self.write(msg, width=80)
        self.scroll_end(animate=False)

    def add_thinking(self, text: str) -> None:
        msg = Text()
        msg.append("⟳ ", style=YELLOW)
        msg.append(text, style=f"italic {DIM}")
        self.write(msg, width=80)
        self.scroll_end(animate=False)

    def add_plan_link(self, plan_id: str, version: int) -> None:
        msg = Text()
        msg.append("◆ ", style=CYAN)
        msg.append(f"Plan drafted: ", style="white")
        msg.append(f"{plan_id} v{version}", style=f"bold {CYAN}")
        msg.append(" — type 'tab' to view details", style=DIM)
        self.write(msg, width=80)
        self.scroll_end(animate=False)

    def add_critic_verdict(self, verdict: str, prose: str) -> None:
        color = GREEN if verdict == "approve" else YELLOW if verdict == "revise" else RED
        msg = Text()
        msg.append("◇ ", style=color)
        msg.append(f"Critic Verdict: ", style="white")
        msg.append(verdict.upper(), style=f"bold {color}")
        self.write(msg, width=80)
        if prose:
            detail = Text(f"  {prose}", style=DIM)
            self.write(detail, width=80)
        self.scroll_end(animate=False)

    def add_awaiting_approval(self, plan_id: str) -> None:
        msg = Text()
        msg.append("◆ ", style=YELLOW)
        msg.append("Awaiting approval for ", style="white")
        msg.append(plan_id, style=f"bold {YELLOW}")
        msg.append(" — type 'approve' or 'cancel'", style=DIM)
        self.write(msg, width=80)
        self.scroll_end(animate=False)

    def add_event(self, event_type: str, text: str) -> None:
        msg = Text()
        msg.append(f"  [{event_type}] ", style=DIM)
        msg.append(text, style="white")
        self.write(msg, width=80)
        self.scroll_end(animate=False)


class PlanPreview(Static):
    plan_data: reactive[dict] = reactive({})
    expanded: reactive[bool] = reactive(False)

    def update_plan(self, data: dict) -> None:
        self.plan_data = data
        self.refresh()

    def clear_plan(self) -> None:
        self.plan_data = {}
        self.refresh()

    def toggle_expand(self) -> None:
        self.expanded = not self.expanded
        self.refresh()

    def render(self) -> Panel:
        if not self.plan_data:
            return Panel(Text("No active plan", style=DIM), title="[bold]Plan Preview[/bold] [dim](P)[/dim]", border_style=DIM, padding=(1, 2), expand=True)

        plan_id = self.plan_data.get("plan_id", "?")
        version = self.plan_data.get("version", "?")
        overview = self.plan_data.get("overview", "")
        tasks = self.plan_data.get("tasks", [])
        models = self.plan_data.get("model_assignments", {})
        sandbox = self.plan_data.get("sandbox_config", {})

        content = Text()
        content.append(f"Plan: ", style="white")
        content.append(f"{plan_id} v{version}\n", style=f"bold {CYAN}")

        if not self.expanded:
            content.append(f"  {len(tasks)} tasks • ", style=DIM)
            content.append("[Press P to expand]", style=f"dim {CYAN}")
        else:
            content.append("─" * 40 + "\n", style=DIM)
            if overview:
                for line in overview.split("\n")[:6]:
                    content.append(f"{line}\n", style="white")
                content.append("\n")
            content.append("Task Graph:\n", style=f"bold {PURPLE}")
            for task in tasks:
                tid = task.get("id", "?")
                title = task.get("title", "?")
                deps = task.get("deps", [])
                tier = task.get("model_tier", "fast")
                budget = task.get("tool_budget", 0)
                tier_color = {"fast": GREEN, "mid": YELLOW, "flagship": RED}.get(tier, DIM)
                content.append(f"  [{tid}]", style=f"bold {tier_color}")
                content.append(f" {title}", style="white")
                if deps:
                    content.append(f" (deps: {', '.join(deps)})", style=DIM)
                content.append(f" ⚡{budget} {tier}\n", style=f"dim {tier_color}")
            content.append("\nConfiguration:\n", style=f"bold {PURPLE}")
            content.append(f"  Sandbox: {sandbox.get('image', 'N/A')} RAM: {sandbox.get('ram_mb', 'N/A')}MB\n", style=DIM)
            content.append(f"  Conductor: {models.get('conductor', 'N/A')}\n", style=DIM)
            content.append(f"  Captain: {models.get('captain', 'N/A')}\n", style=DIM)
            content.append(f"  Crew: {models.get('crew', 'N/A')}\n", style=DIM)

        return Panel(content, title="[bold]Plan Preview[/bold] [dim](P)[/dim]", border_style=CYAN, padding=(1, 2), expand=True)


class CriticPanel(Static):
    state: reactive[str] = reactive("idle")
    verdict_data: reactive[dict] = reactive({})
    expanded: reactive[bool] = reactive(False)

    def set_reviewing(self, plan_id: str, version: int) -> None:
        self.state = "reviewing"
        self.verdict_data = {"plan_id": plan_id, "version": version}
        self.refresh()

    def set_thinking(self, text: str) -> None:
        self.state = "thinking"
        self.verdict_data = {**self.verdict_data, "thinking": text}
        self.refresh()

    def set_verdict(self, verdict: str, issues: list, prose: str) -> None:
        self.state = "done"
        self.verdict_data = {**self.verdict_data, "verdict": verdict, "issues": issues, "prose": prose}
        self.refresh()

    def clear(self) -> None:
        self.state = "idle"
        self.verdict_data = {}
        self.refresh()

    def toggle_expand(self) -> None:
        self.expanded = not self.expanded
        self.refresh()

    def render(self) -> Panel:
        content = Text()
        if self.state == "idle":
            content.append("Critic standing by", style=DIM)
        elif self.state == "reviewing":
            content.append("⟳ ", style=YELLOW)
            content.append(f"Reviewing {self.verdict_data.get('plan_id', '?')}...", style=f"italic {YELLOW}")
        elif self.state == "thinking":
            content.append("⟳ ", style=YELLOW)
            content.append(self.verdict_data.get("thinking", ""), style=f"italic {DIM}")
        elif self.state == "done":
            verdict = self.verdict_data.get("verdict", "?")
            issues = self.verdict_data.get("issues", [])
            prose = self.verdict_data.get("prose", "")
            color = GREEN if verdict == "approve" else YELLOW if verdict == "revise" else RED
            icon = "✓" if verdict == "approve" else "⚠" if verdict == "revise" else "✗"
            content.append(f"{icon} ", style=color)
            content.append(f"Verdict: ", style="white")
            content.append(f"{verdict.upper()}\n", style=f"bold {color}")
            if not self.expanded:
                content.append(f"  {len(issues)} issues • ", style=DIM)
                content.append("[Press C to expand]", style=f"dim {YELLOW}")
            else:
                if prose:
                    content.append(f"\n{prose}\n", style="white")
                if issues:
                    content.append("\nIssues:\n", style=f"bold {PURPLE}")
                    for issue in issues:
                        sev = issue.get("severity", "info")
                        sev_color = {"info": CYAN, "warn": YELLOW, "block": RED}.get(sev, DIM)
                        sev_icon = {"info": "ℹ", "warn": "⚠", "block": "✗"}.get(sev, "•")
                        content.append(f"  {sev_icon} ", style=sev_color)
                        content.append(f"[{issue.get('section', '?')}] {issue.get('message', '')}\n", style="white")
                        if issue.get("suggested_fix"):
                            content.append(f"    → {issue['suggested_fix']}\n", style=f"italic {DIM}")
        return Panel(content, title="[bold]Critic Review[/bold] [dim](C)[/dim]", border_style=YELLOW, padding=(1, 2), expand=True)


class CaptainPanel(Static):
    events: reactive[list] = reactive([])
    status: reactive[str] = reactive("idle")

    def add_event(self, event_type: str, data: dict) -> None:
        self.events = self.events + [(event_type, data)]
        self.refresh()

    def set_status(self, status: str) -> None:
        self.status = status
        self.refresh()

    def clear(self) -> None:
        self.events = []
        self.status = "idle"
        self.refresh()

    def render(self) -> Panel:
        content = Text()
        status_color = {"idle": DIM, "running": GREEN, "completed": GREEN, "failed": RED}.get(self.status, DIM)
        content.append("Status: ", style="white")
        content.append(f"{self.status.upper()}\n", style=f"bold {status_color}")
        content.append("─" * 40 + "\n", style=DIM)

        if not self.events:
            content.append("No activity yet", style=DIM)
        else:
            for event_type, data in self.events[-30:]:
                if event_type == "crew_started":
                    content.append("→ ", style=GREEN)
                    content.append(f"{data.get('agent_id', '?')}", style=f"bold {GREEN}")
                    content.append(f" started: {data.get('task_title', '?')}\n", style="white")
                elif event_type == "crew_tool_summary":
                    icon = "✓" if data.get("success", True) else "✗"
                    color = GREEN if data.get("success", True) else RED
                    content.append(f"  {icon} ", style=color)
                    content.append(f"{data.get('text', '')}\n", style="white")
                elif event_type == "crew_done":
                    content.append("✓ ", style=GREEN)
                    content.append(f"{data.get('agent_id', '?')}", style=f"bold {GREEN}")
                    content.append(f" completed: {data.get('summary', '')}\n", style="white")
                elif event_type == "crew_failed":
                    content.append("✗ ", style=RED)
                    content.append(f"{data.get('agent_id', '?')}", style=f"bold {RED}")
                    content.append(f" failed: {data.get('reason', '')}\n", style="white")
                elif event_type == "merge_started":
                    content.append("⟳ ", style=YELLOW)
                    content.append(f"Merging {data.get('agent_id', '?')}...\n", style=DIM)
                elif event_type == "merge_completed":
                    content.append("✓ ", style=GREEN)
                    content.append(f"Merged {data.get('agent_id', '?')}", style=GREEN)
                    if data.get("changed_paths"):
                        content.append(f" ({', '.join(data['changed_paths'])})", style=DIM)
                    content.append("\n", style="white")
                elif event_type == "captain_started":
                    content.append("▶ ", style=CYAN)
                    content.append(f"Captain started, sandbox: {data.get('sandbox_id', '?')}\n", style=f"bold {CYAN}")
                elif event_type == "plan_completed":
                    content.append("\n◆ ", style=GREEN)
                    content.append("PLAN COMPLETED SUCCESSFULLY\n", style=f"bold {GREEN}")
                elif event_type == "plan_failed":
                    content.append("\n◆ ", style=RED)
                    content.append(f"PLAN FAILED: {data.get('reason', 'Unknown')}\n", style=f"bold {RED}")

        return Panel(content, title="[bold]Captain Panel[/bold]", border_style=PURPLE, padding=(1, 2), expand=True)


# ============================================================
# MAIN CLI
# ============================================================

class MiniLogo(Widget):
    frame: reactive[int] = reactive(0)

    def on_mount(self) -> None:
        self.set_interval(0.6, self.tick)

    def tick(self) -> None:
        self.frame += 1

    def render(self) -> Text:
        f = self.frame % 8
        glow_colors = ["#6d28d9", "#7c3aed", "#8b5cf6", "#a855f7", "#8b5cf6", "#7c3aed", "#6d28d9", "#5b21b6"]
        glow = glow_colors[f % len(glow_colors)]
        art = Text()
        art.append("  ███╗   ███╗  \n", style=f"bold {glow}")
        art.append("  ████╗ ████║  \n", style=f"bold {glow}")
        art.append("  ██╔████╔██║  \n", style=f"bold #8b5cf6")
        art.append("  ██║╚██╔╝██║  \n", style=f"bold #7c3aed")
        art.append("  ██║ ╚═╝ ██║  \n", style=f"bold #6d28d9")
        art.append("  ╚═╝     ╚═╝  \n", style=f"bold #5b21b6")
        return art


class CLIHeader(Static):
    def render(self) -> Panel:
        header_text = Text()
        header_text.append("Welcome to ", style="white")
        header_text.append("MowisAI", style=f"bold {PURPLE}")
        header_text.append("\n", style="white")
        header_text.append("multi-agent conductor system", style="dim white")
        return Panel(header_text, border_style=PURPLE, padding=(1, 2), expand=False)


class StatusSection(Static):
    def render(self) -> Text:
        status = Text()
        status.append("● ", style=PURPLE)
        status.append("Connected to MCP Server\n", style="white")
        status.append("● ", style=PURPLE)
        status.append("Logged in as user: ", style="white")
        status.append("MowisAgent\n", style=f"bold {PURPLE}")
        status.append("\n", style="white")
        status.append(os.path.expanduser("~"), style="dim white")
        status.append(" [main]", style="dim white")
        return status


class Line(Static):
    def render(self) -> Text:
        width = max(10, self.size.width)
        return Text("─" * width, style=PURPLE)


SLASH_COMMANDS = {"/help": "Show commands", "/clear": "Clear chat", "/quit": "Exit", "/about": "About"}
SAMPLE_FILES = ["main.py", "config.json", "utils.py", "README.md", "requirements.txt", "models/user.py", "src/app.py", "tests/test_main.py"]


class SlashMenu(Static):
    def __init__(self, **kwargs):
        super().__init__(**kwargs)
        self.selected = 0
        self.visible = False
        self.commands = []

    def show_menu(self, filter_text=""):
        self.commands = [(c, d) for c, d in SLASH_COMMANDS.items() if c.startswith(filter_text.lower())] if filter_text else list(SLASH_COMMANDS.items())
        self.selected = 0
        self.visible = bool(self.commands)
        self.display = self.visible
        self.refresh()

    def hide_menu(self):
        self.visible = False
        self.display = False
        self.refresh()

    def move_up(self):
        if self.commands:
            self.selected = (self.selected - 1) % len(self.commands)
            self.refresh()

    def move_down(self):
        if self.commands:
            self.selected = (self.selected + 1) % len(self.commands)
            self.refresh()

    def get_selected(self):
        return self.commands[self.selected][0] if self.commands else ""

    def render(self) -> Text:
        if not self.visible or not self.commands:
            return Text()
        t = Text()
        for i, (cmd, desc) in enumerate(self.commands):
            if i == self.selected:
                t.append(f"  {cmd:<12}", style=f"bold {PURPLE} on #1a1a2e")
                t.append(f" {desc}", style=f"white on #1a1a2e")
            else:
                t.append(f"  {cmd:<12}", style=PURPLE)
                t.append(f" {desc}", style="dim white")
            if i < len(self.commands) - 1:
                t.append("\n")
        return t


class FileMenu(Static):
    def __init__(self, **kwargs):
        super().__init__(**kwargs)
        self.selected = 0
        self.visible = False
        self.files = []

    def show_menu(self, filter_text=""):
        self.files = [f for f in SAMPLE_FILES if filter_text.lower() in f.lower()] if filter_text else list(SAMPLE_FILES)
        self.selected = 0
        self.visible = bool(self.files)
        self.display = self.visible
        self.refresh()

    def hide_menu(self):
        self.visible = False
        self.display = False
        self.refresh()

    def move_up(self):
        if self.files:
            self.selected = (self.selected - 1) % len(self.files)
            self.refresh()

    def move_down(self):
        if self.files:
            self.selected = (self.selected + 1) % len(self.files)
            self.refresh()

    def get_selected(self):
        return self.files[self.selected] if self.files else ""

    def render(self) -> Text:
        if not self.visible or not self.files:
            return Text()
        t = Text()
        for i, f in enumerate(self.files):
            if i == self.selected:
                t.append(f"  @ ", style=f"bold {CYAN} on #1a1a2e")
                t.append(f"{f:<24}", style=f"bold white on #1a1a2e")
            else:
                t.append(f"  @ ", style=CYAN)
                t.append(f"{f}", style="dim white")
            if i < len(self.files) - 1:
                t.append("\n")
        return t


class CLIInput(Input):
    def __init__(self, **kwargs):
        super().__init__(**kwargs)
        self._skip_submit = 0

    def on_input_changed(self, event):
        value = event.value or ""
        try:
            slash_menu = self.screen.query_one(SlashMenu)
            file_menu = self.screen.query_one(FileMenu)
            if value.startswith("/"):
                slash_menu.show_menu(value)
                file_menu.hide_menu()
            elif "@" in value:
                file_menu.show_menu(value[value.rindex("@") + 1:])
                slash_menu.hide_menu()
            else:
                slash_menu.hide_menu()
                file_menu.hide_menu()
        except Exception:
            pass

    def on_key(self, event):
        try:
            slash_menu = self.screen.query_one(SlashMenu)
            file_menu = self.screen.query_one(FileMenu)
            active = slash_menu if slash_menu.visible else (file_menu if file_menu.visible else None)
            if active:
                if event.key == "up":
                    active.move_up()
                    event.prevent_default()
                elif event.key == "down":
                    active.move_down()
                    event.prevent_default()
                elif event.key == "enter":
                    sel = active.get_selected()
                    if sel:
                        if isinstance(active, SlashMenu):
                            self.value = sel
                            active.hide_menu()
                            self._exec_cmd(sel)
                        else:
                            self._skip_submit += 1
                            v = self.value or ""
                            self.value = v[:v.rindex("@")] + "@" + sel + " "
                            active.hide_menu()
                        event.prevent_default()
                elif event.key == "escape":
                    active.hide_menu()
                    event.prevent_default()
        except Exception:
            pass

    def _exec_cmd(self, cmd):
        ml = self.screen.query_one(MessageLog)
        if cmd == "/help":
            ml.add_chat("Commands: /help /clear /quit /about", "conductor")
        elif cmd == "/clear":
            ml.clear()
        elif cmd == "/quit":
            self.app.exit()
        elif cmd == "/about":
            ml.add_chat("MowisAI v1.0", "conductor")
        self.value = ""

    def on_input_submitted(self, event):
        if self._skip_submit > 0:
            self._skip_submit -= 1
            return
        value = (event.value or "").strip()
        if value:
            if value.startswith("/"):
                self._exec_cmd(value)
            else:
                try:
                    self.screen.handle_user_message(value)
                except Exception:
                    pass
        self.value = ""
        self.focus()


class HelpFooter(Static):
    def render(self) -> Text:
        f = Text()
        f.append("Tab", style=f"bold {PURPLE}")
        f.append(" Overlay • ", style="white")
        f.append("P", style=f"bold {PURPLE}")
        f.append(" Plan • ", style="white")
        f.append("C", style=f"bold {PURPLE}")
        f.append(" Critic • ", style="white")
        f.append("Ctrl+c", style=f"bold {PURPLE}")
        f.append(" Exit", style="white")
        return f


# ============================================================
# SIMULATION ENGINE
# ============================================================

class SimulationEngine:
    def __init__(self, callback):
        self.callback = callback
        self.plan_counter = 0
        self.state = "idle"
        self.current_plan = None

    async def emit(self, event_type, data):
        await self.callback(event_type, data)

    async def handle_message(self, text):
        lower = text.lower().strip()
        if self.state == "idle":
            if any(w in lower for w in ["hello", "hi", "hey"]):
                await self.emit("conductor_reply", {"kind": "chat", "text": "Hey! I'm doing great, thanks for asking. I'm MowisAI, your multi-agent coding assistant. What would you like to work on today?"})
            elif any(w in lower for w in ["work on", "build", "create", "make", "help me", "i want", "i need", "can you"]):
                await self._draft_plan(text)
            else:
                await self.emit("conductor_reply", {"kind": "chat", "text": f"I understand you said: '{text}'. Could you tell me what you'd like to work on?"})
        elif self.state == "awaiting":
            if "approve" in lower or "yes" in lower or "start" in lower:
                await self.emit("user_approved", {"plan_id": self.current_plan["id"]})
                await self.emit("conductor_reply", {"kind": "chat", "text": "Plan approved! Starting execution..."})
                await asyncio.sleep(1)
                await self._execute_plan()
            elif "cancel" in lower:
                await self.emit("user_cancelled", {"plan_id": self.current_plan["id"]})
                self.state = "idle"
                self.current_plan = None
                await self.emit("conductor_reply", {"kind": "chat", "text": "Plan cancelled."})
            else:
                await self.emit("conductor_reply", {"kind": "chat", "text": "Type 'approve' or 'cancel'."})

    async def _draft_plan(self, goal):
        self.plan_counter += 1
        pid = f"plan-{self.plan_counter}"
        await self.emit("conductor_reply", {"kind": "thinking", "text": "Analyzing your request and drafting a plan..."})
        await asyncio.sleep(1.5)

        tasks = self._gen_tasks(goal)
        overview = f"## Plan: {goal}\n\nThis plan breaks down your request into {len(tasks)} tasks.\nEach task runs in an isolated sandbox with specialized agents."

        self.current_plan = {"id": pid, "tasks": tasks}
        await self.emit("plan_drafted", {"plan_id": pid, "version": 1, "overview": overview, "tasks": tasks, "sandbox_config": {"image": "ubuntu-24.04", "ram_mb": 8192, "cpu_millis": 4000}, "model_assignments": {"conductor": "claude-opus-4-7", "captain": "claude-sonnet-4-6", "crew": "claude-haiku-4-5"}})
        await asyncio.sleep(0.5)
        await self._run_critic(pid)

    def _gen_tasks(self, goal):
        lower = goal.lower()
        if any(w in lower for w in ["api", "endpoint", "server"]):
            return [
                {"id": "t1", "title": "Set up project structure", "description": "Create directories and config files", "deps": [], "model_tier": "fast", "tool_budget": 10, "files_hint": ["package.json", "src/"]},
                {"id": "t2", "title": "Implement API routes", "description": "Create main API endpoints", "deps": ["t1"], "model_tier": "mid", "tool_budget": 20, "files_hint": ["src/routes/"]},
                {"id": "t3", "title": "Add data models", "description": "Define schemas and validation", "deps": ["t1"], "model_tier": "mid", "tool_budget": 15, "files_hint": ["src/models/"]},
                {"id": "t4", "title": "Implement middleware", "description": "Auth, logging, error handling", "deps": ["t2"], "model_tier": "mid", "tool_budget": 15, "files_hint": ["src/middleware/"]},
                {"id": "t5", "title": "Write tests", "description": "Unit and integration tests", "deps": ["t2", "t3"], "model_tier": "fast", "tool_budget": 20, "files_hint": ["tests/"]},
            ]
        return [
            {"id": "t1", "title": "Analyze requirements", "description": "Review request and plan approach", "deps": [], "model_tier": "mid", "tool_budget": 10, "files_hint": ["README.md"]},
            {"id": "t2", "title": "Set up project", "description": "Create structure and config", "deps": ["t1"], "model_tier": "fast", "tool_budget": 10, "files_hint": ["src/"]},
            {"id": "t3", "title": "Implement core", "description": "Build main features", "deps": ["t2"], "model_tier": "mid", "tool_budget": 25, "files_hint": ["src/"]},
            {"id": "t4", "title": "Add tests", "description": "Write tests and docs", "deps": ["t3"], "model_tier": "fast", "tool_budget": 15, "files_hint": ["tests/"]},
        ]

    async def _run_critic(self, pid):
        await self.emit("critic_reviewing", {"plan_id": pid, "version": 1})
        await asyncio.sleep(2)
        await self.emit("critic_thinking", {"text": "Reviewing plan structure and dependencies..."})
        await asyncio.sleep(2)
        await self.emit("critic_verdict", {"plan_id": pid, "version": 1, "verdict": "approve", "issues": [{"severity": "info", "section": "tasks.toml", "message": "Task dependencies valid", "suggested_fix": None}, {"severity": "warn", "section": "sandbox.toml", "message": "Consider more RAM for large builds", "suggested_fix": "Set ram_mb = 16384"}], "prose": "Plan is well-structured with clear task boundaries."})
        self.state = "awaiting"
        await self.emit("conductor_reply", {"kind": "awaiting", "text": "Plan reviewed. Type 'approve' to start.", "plan_id": pid})

    async def _execute_plan(self):
        if not self.current_plan:
            return
        self.state = "running"
        sid = f"sandbox-{self.current_plan['id']}"
        await self.emit("captain_started", {"plan_id": self.current_plan["id"], "sandbox_id": sid})

        done = set()
        while len(done) < len(self.current_plan["tasks"]):
            ready = [t for t in self.current_plan["tasks"] if t["id"] not in done and all(d in done for d in t["deps"])]
            if not ready:
                break
            for task in ready:
                await self._exec_task(task, sid)
                done.add(task["id"])

        self.state = "idle"
        await self.emit("plan_completed", {"plan_id": self.current_plan["id"], "sandbox_id": sid})
        await self.emit("conductor_reply", {"kind": "chat", "text": "All tasks completed! Let me know if you need anything else."})
        self.current_plan = None

    async def _exec_task(self, task, sid):
        aid = f"ag-{task['id']}"
        await self.emit("crew_started", {"plan_id": self.current_plan["id"], "task_id": task["id"], "agent_id": aid, "task_title": task["title"]})
        await asyncio.sleep(0.5)

        for tc in self._gen_tool_calls(task):
            await asyncio.sleep(random.uniform(0.8, 2.0))
            await self.emit("crew_tool_summary", {"agent_id": aid, "task_id": task["id"], "tool_name": tc["tool"], "text": tc["summary"], "success": True})

        await asyncio.sleep(0.5)
        await self.emit("crew_done", {"plan_id": self.current_plan["id"], "task_id": task["id"], "agent_id": aid, "summary": f"Completed: {task['title']}", "tool_calls": 3})
        await asyncio.sleep(0.3)
        await self.emit("merge_started", {"plan_id": self.current_plan["id"], "agent_id": aid})
        await asyncio.sleep(0.8)
        await self.emit("merge_completed", {"plan_id": self.current_plan["id"], "agent_id": aid, "changed_paths": task.get("files_hint", ["src/main.py"])[:2]})

    def _gen_tool_calls(self, task):
        lower = (task["title"] + " " + task["description"]).lower()
        if "set up" in lower or "create" in lower:
            return [{"tool": "create_directory", "summary": "Agent created directory src/"}, {"tool": "write_file", "summary": "Agent wrote 24 lines to src/main.py"}, {"tool": "run_command", "summary": "Agent ran `pip install -r requirements.txt`"}]
        elif "implement" in lower or "build" in lower or "core" in lower:
            return [{"tool": "read_file", "summary": "Agent read src/main.py (1.2 KB)"}, {"tool": "write_file", "summary": "Agent wrote 85 lines to src/routes.py"}, {"tool": "run_command", "summary": "Agent ran `python -m pytest tests/ -v`"}]
        elif "test" in lower:
            return [{"tool": "read_file", "summary": "Agent read src/routes.py (3.4 KB)"}, {"tool": "write_file", "summary": "Agent wrote 56 lines to tests/test_routes.py"}, {"tool": "run_command", "summary": "Agent ran `python -m pytest tests/ -v --cov`"}]
        return [{"tool": "read_file", "summary": "Agent read project structure"}, {"tool": "write_file", "summary": f"Agent wrote implementation for {task['title']}"}, {"tool": "run_command", "summary": "Agent ran validation checks"}]


# ============================================================
# MAIN CLI SCREEN
# ============================================================

class MainCLI(Screen):
    CSS = """
    MainCLI {
        background: #000000;
        color: #e0e0e0;
    }
    #main-view {
        height: 1fr;
        width: 1fr;
    }
    #main-view.hidden {
        display: none;
    }
    #overlay-view {
        height: 1fr;
        width: 1fr;
        display: none;
        overflow-y: auto;
        scrollbar-size-vertical: 0;
    }
    #overlay-view.visible {
        display: block;
    }
    #scroll-area {
        height: 1fr;
        overflow-y: auto;
        scrollbar-size-vertical: 0;
    }
    #header-row {
        height: auto;
        width: 1fr;
    }
    MiniLogo {
        width: 18;
        height: 7;
    }
    MiniLogo.hidden {
        display: none;
    }
    CLIHeader {
        width: 1fr;
        height: auto;
    }
    StatusSection {
        height: auto;
        margin-bottom: 1;
        padding: 0 2;
    }
    MessageLog {
        height: 1fr;
        min-height: 100%;
        padding: 1 2;
        background: #000000;
        width: 1fr;
        overflow-x: hidden;
    }
    #slash-menu {
        height: auto;
        max-height: 10;
        display: none;
        background: #0d0d1a;
        border: solid #6d28d9;
        margin: 0 2;
        padding: 0 1;
    }
    #file-menu {
        height: auto;
        max-height: 10;
        display: none;
        background: #0d0d1a;
        border: solid #22d3ee;
        margin: 0 2;
        padding: 0 1;
    }
    .cli-input {
        margin: 0 2;
        margin-bottom: 0;
        border: none;
        padding: 0 1;
        height: 1;
        background: #000000;
        color: #ffffff;
    }
    HelpFooter {
        height: auto;
        padding: 0 2;
        margin-top: 1;
        background: #000000;
    }
    Vertical {
        height: 1fr;
        width: 1fr;
    }
    """

    BINDINGS = [
        ("ctrl+c", "quit", "Exit"),
        ("tab", "toggle_overlay", "Overlay"),
        ("p", "toggle_plan", "Plan"),
        ("c", "toggle_critic", "Critic"),
    ]

    def __init__(self, **kwargs):
        super().__init__(**kwargs)
        self.simulation = None
        self.overlay_visible = False

    def compose(self) -> ComposeResult:
        yield Vertical(
            VerticalScroll(
                Horizontal(MiniLogo(id="mini-logo"), CLIHeader(), id="header-row"),
                StatusSection(),
                MessageLog(id="message-log"),
                id="scroll-area"
            ),
            Line(),
            SlashMenu(id="slash-menu"),
            FileMenu(id="file-menu"),
            CLIInput(id="command-input", placeholder="Type a message, @ for files, / for commands", classes="cli-input"),
            Line(),
            HelpFooter(),
            id="main-view"
        )
        yield VerticalScroll(
            PlanPreview(id="overlay-plan-preview"),
            CriticPanel(id="overlay-critic-panel"),
            CaptainPanel(id="overlay-captain-panel"),
            id="overlay-view"
        )

    def on_mount(self) -> None:
        self.simulation = SimulationEngine(self._handle_event)
        try:
            self.query_one("#command-input", CLIInput).focus()
        except Exception:
            pass

    def handle_user_message(self, text):
        self.query_one(MessageLog).add_chat(text, "user")
        if self.simulation:
            asyncio.create_task(self.simulation.handle_message(text))

    async def _handle_event(self, event_type, data):
        try:
            ml = self.query_one(MessageLog)
            pp = self.query_one("#overlay-plan-preview")
            cp = self.query_one("#overlay-critic-panel")
            cap = self.query_one("#overlay-captain-panel")

            if event_type == "conductor_reply":
                kind = data.get("kind", "chat")
                if kind == "chat":
                    ml.add_chat(data["text"], "conductor")
                elif kind == "thinking":
                    ml.add_thinking(data["text"])
                elif kind == "awaiting":
                    ml.add_awaiting_approval(data.get("plan_id", "?"))
                elif kind == "hot_patch":
                    ml.add_chat(data["text"], "conductor")
            elif event_type == "plan_drafted":
                pp.update_plan(data)
                ml.add_plan_link(data.get("plan_id", "?"), data.get("version", 1))
            elif event_type == "critic_reviewing":
                cp.set_reviewing(data.get("plan_id", "?"), data.get("version", 1))
            elif event_type == "critic_thinking":
                cp.set_thinking(data.get("text", ""))
            elif event_type == "critic_verdict":
                cp.set_verdict(data.get("verdict", "?"), data.get("issues", []), data.get("prose", ""))
                ml.add_critic_verdict(data.get("verdict", "?"), data.get("prose", ""))
            elif event_type == "user_approved":
                ml.add_event("system", f"User approved {data.get('plan_id', '?')}")
            elif event_type == "user_cancelled":
                ml.add_event("system", f"User cancelled {data.get('plan_id', '?')}")
                pp.clear_plan()
                cap.clear()
                cp.clear()
            elif event_type == "captain_started":
                cap.set_status("running")
                cap.add_event(event_type, data)
                self._show_overlay()
            elif event_type in ["crew_started", "crew_tool_summary", "crew_done", "crew_failed", "merge_started", "merge_completed"]:
                cap.add_event(event_type, data)
            elif event_type == "plan_completed":
                cap.set_status("completed")
                cap.add_event(event_type, data)
                ml.add_chat("Plan completed successfully!", "system")
            elif event_type == "plan_failed":
                cap.set_status("failed")
                cap.add_event(event_type, data)
                ml.add_chat(f"Plan failed: {data.get('reason', 'Unknown')}", "system")
            self.refresh()
        except Exception:
            pass

    def on_scroll(self, event):
        try:
            sa = self.query_one("#scroll-area")
            ml = self.query_one("#mini-logo")
            ml.set_class(sa.scroll_y > 0, "hidden")
        except Exception:
            pass

    def action_quit(self):
        self.app.exit()

    def action_toggle_overlay(self):
        self.overlay_visible = not self.overlay_visible
        try:
            self.query_one("#main-view").set_class(self.overlay_visible, "hidden")
            self.query_one("#overlay-view").set_class(self.overlay_visible, "visible")
            if not self.overlay_visible:
                self.query_one("#command-input", CLIInput).focus()
        except Exception:
            pass

    def action_toggle_plan(self):
        try:
            self.query_one("#overlay-plan-preview").toggle_expand()
        except Exception:
            pass

    def action_toggle_critic(self):
        try:
            self.query_one("#overlay-critic-panel").toggle_expand()
        except Exception:
            pass

    def _show_overlay(self):
        if not self.overlay_visible:
            self.overlay_visible = True
            try:
                self.query_one("#main-view").set_class(True, "hidden")
                self.query_one("#overlay-view").set_class(True, "visible")
            except Exception:
                pass


# ============================================================
# APP
# ============================================================

class MowisAI(App):
    TITLE = "MowisAI"
    SUB_TITLE = "multi-agent conductor"
    THEME = "nord"
    CSS = """
    Screen {
        background: #000000;
        color: #e0e0e0;
    }
    """

    def on_mount(self):
        self.title = "MowisAI"
        self.sub_title = "multi-agent conductor"
        self.push_screen(SplashScreen())

    def on_key(self, event):
        if isinstance(self.screen, SplashScreen) and event.key == "enter":
            self.switch_screen(SetupWizard())

    def compose(self) -> ComposeResult:
        yield Static("")


if __name__ == "__main__":
    app = MowisAI()
    app.run()
