//go:build !headless

package cmd

import (
	"context"
	"fmt"
	"os"
	"sync"
	"time"

	tea "github.com/charmbracelet/bubbletea"
	zone "github.com/lrstanley/bubblezone"
	"github.com/mowisai/mowis-agent/internal/app"
	"github.com/mowisai/mowis-agent/internal/format"
	"github.com/mowisai/mowis-agent/internal/logging"
	"github.com/mowisai/mowis-agent/internal/pubsub"
	"github.com/mowisai/mowis-agent/internal/tui"
	"github.com/mowisai/mowis-agent/internal/version"
	"github.com/spf13/cobra"
)

var rootCmd = &cobra.Command{
	Use:   "opencode",
	Short: "Terminal-based AI assistant for software development",
	Long: `OpenCode is a powerful terminal-based AI assistant that helps with software development tasks.
It provides an interactive chat interface with AI capabilities, code analysis, and LSP integration
to assist developers in writing, debugging, and understanding code directly from the terminal.`,
	Example: `
  # Run in interactive mode
  opencode

  # Run with debug logging
  opencode -d

  # Run with debug logging in a specific directory
  opencode -d -c /path/to/project

  # Print version
  opencode -v

  # Run a single non-interactive prompt
  opencode -p "Explain the use of context in Go"

  # Run a single non-interactive prompt with JSON output format
  opencode -p "Explain the use of context in Go" -f json
  `,
	RunE: func(cmd *cobra.Command, args []string) error {
		if cmd.Flag("help").Changed {
			cmd.Help()
			return nil
		}
		if cmd.Flag("version").Changed {
			fmt.Println(version.Version)
			return nil
		}

		flags := parseFlags(cmd)
		a, _, cancel, err := initApp(flags)
		if err != nil {
			return err
		}
		defer cancel()
		defer a.Shutdown()

		// Non-interactive mode
		if flags.Prompt != "" {
			return a.RunNonInteractive(context.Background(), flags.Prompt, flags.OutputFormat, flags.Quiet)
		}

		// Interactive TUI mode
		zone.NewGlobal()
		program := tea.NewProgram(
			tui.New(a),
			tea.WithAltScreen(),
		)

		ch, cancelSubs := setupSubscriptions(a, context.Background())

		tuiCtx, tuiCancel := context.WithCancel(context.Background())
		var tuiWg sync.WaitGroup
		tuiWg.Add(1)

		go func() {
			defer tuiWg.Done()
			defer logging.RecoverPanic("TUI-message-handler", func() {
				attemptTUIRecovery(program)
			})

			for {
				select {
				case <-tuiCtx.Done():
					logging.Info("TUI message handler shutting down")
					return
				case msg, ok := <-ch:
					if !ok {
						logging.Info("TUI message channel closed")
						return
					}
					program.Send(msg)
				}
			}
		}()

		cleanup := func() {
			a.Shutdown()
			cancelSubs()
			tuiCancel()
			tuiWg.Wait()
			logging.Info("All goroutines cleaned up")
		}

		result, err := program.Run()
		cleanup()

		if err != nil {
			logging.Error("TUI error: %v", err)
			return fmt.Errorf("TUI error: %v", err)
		}

		logging.Info("TUI exited with result: %v", result)
		return nil
	},
}

func attemptTUIRecovery(program *tea.Program) {
	logging.Info("Attempting to recover TUI after panic")
	program.Quit()
}

func setupSubscriber[T any](
	ctx context.Context,
	wg *sync.WaitGroup,
	name string,
	subscriber func(context.Context) <-chan pubsub.Event[T],
	outputCh chan<- tea.Msg,
) {
	wg.Add(1)
	go func() {
		defer wg.Done()
		defer logging.RecoverPanic(fmt.Sprintf("subscription-%s", name), nil)

		subCh := subscriber(ctx)

		for {
			select {
			case event, ok := <-subCh:
				if !ok {
					logging.Info("subscription channel closed", "name", name)
					return
				}

				var msg tea.Msg = event

				select {
				case outputCh <- msg:
				case <-time.After(2 * time.Second):
					logging.Warn("message dropped due to slow consumer", "name", name)
				case <-ctx.Done():
					logging.Info("subscription cancelled", "name", name)
					return
				}
			case <-ctx.Done():
				logging.Info("subscription cancelled", "name", name)
				return
			}
		}
	}()
}

func setupSubscriptions(a *app.App, parentCtx context.Context) (chan tea.Msg, func()) {
	ch := make(chan tea.Msg, 100)

	wg := sync.WaitGroup{}
	ctx, cancel := context.WithCancel(parentCtx)

	setupSubscriber(ctx, &wg, "logging", logging.Subscribe, ch)
	setupSubscriber(ctx, &wg, "sessions", a.Sessions.Subscribe, ch)
	setupSubscriber(ctx, &wg, "messages", a.Messages.Subscribe, ch)
	setupSubscriber(ctx, &wg, "permissions", a.Permissions.Subscribe, ch)
	setupSubscriber(ctx, &wg, "coderAgent", a.CoderAgent.Subscribe, ch)

	cleanupFunc := func() {
		logging.Info("Cancelling all subscriptions")
		cancel()

		waitCh := make(chan struct{})
		go func() {
			defer logging.RecoverPanic("subscription-cleanup", nil)
			wg.Wait()
			close(waitCh)
		}()

		select {
		case <-waitCh:
			logging.Info("All subscription goroutines completed successfully")
			close(ch)
		case <-time.After(5 * time.Second):
			logging.Warn("Timed out waiting for some subscription goroutines to complete")
			close(ch)
		}
	}
	return ch, cleanupFunc
}

func Execute() {
	err := rootCmd.Execute()
	if err != nil {
		os.Exit(1)
	}
}

func init() {
	rootCmd.Flags().BoolP("help", "h", false, "Help")
	rootCmd.Flags().BoolP("version", "v", false, "Version")
	rootCmd.Flags().BoolP("debug", "d", false, "Debug")
	rootCmd.Flags().StringP("cwd", "c", "", "Current working directory")
	rootCmd.Flags().StringP("prompt", "p", "", "Prompt to run in non-interactive mode")
	rootCmd.Flags().StringP("output-format", "f", format.Text.String(),
		"Output format for non-interactive mode (text, json)")
	rootCmd.Flags().BoolP("quiet", "q", false, "Hide spinner in non-interactive mode")

	rootCmd.RegisterFlagCompletionFunc("output-format", func(cmd *cobra.Command, args []string, toComplete string) ([]string, cobra.ShellCompDirective) {
		return format.SupportedFormats, cobra.ShellCompDirectiveNoFileComp
	})
}
