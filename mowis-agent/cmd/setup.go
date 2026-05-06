package cmd

import (
	"context"
	"fmt"
	"os"
	"time"

	"github.com/mowisai/mowis-agent/internal/app"
	"github.com/mowisai/mowis-agent/internal/config"
	"github.com/mowisai/mowis-agent/internal/db"
	"github.com/mowisai/mowis-agent/internal/format"
	"github.com/mowisai/mowis-agent/internal/llm/agent"
	"github.com/mowisai/mowis-agent/internal/logging"
	"github.com/spf13/cobra"
)

// commonFlags holds parsed CLI flags shared across modes.
type commonFlags struct {
	Debug        bool
	Cwd          string
	Prompt       string
	OutputFormat string
	Quiet        bool
}

// parseFlags extracts common flags from a cobra command.
func parseFlags(cmd *cobra.Command) commonFlags {
	debug, _ := cmd.Flags().GetBool("debug")
	cwd, _ := cmd.Flags().GetString("cwd")
	prompt, _ := cmd.Flags().GetString("prompt")
	outputFormat, _ := cmd.Flags().GetString("output-format")
	quiet, _ := cmd.Flags().GetBool("quiet")
	return commonFlags{
		Debug:        debug,
		Cwd:          cwd,
		Prompt:       prompt,
		OutputFormat: outputFormat,
		Quiet:        quiet,
	}
}

// initApp loads config, connects DB, creates the App instance.
// Returns the app, working directory, and a cleanup function.
func initApp(flags commonFlags) (*app.App, string, context.CancelFunc, error) {
	if flags.Cwd != "" {
		if err := os.Chdir(flags.Cwd); err != nil {
			return nil, "", nil, fmt.Errorf("failed to change directory: %v", err)
		}
	}
	cwd := flags.Cwd
	if cwd == "" {
		c, err := os.Getwd()
		if err != nil {
			return nil, "", nil, fmt.Errorf("failed to get current working directory: %v", err)
		}
		cwd = c
	}

	if !format.IsValid(flags.OutputFormat) {
		return nil, "", nil, fmt.Errorf("invalid format option: %s\n%s", flags.OutputFormat, format.GetHelpText())
	}

	if _, err := config.Load(cwd, flags.Debug); err != nil {
		return nil, "", nil, err
	}

	conn, err := db.Connect()
	if err != nil {
		return nil, "", nil, err
	}

	ctx, cancel := context.WithCancel(context.Background())

	newApp, err := app.New(ctx, conn)
	if err != nil {
		cancel()
		return nil, "", nil, fmt.Errorf("failed to create app: %w", err)
	}

	go initMCPTools(ctx, newApp)

	return newApp, cwd, cancel, nil
}

func initMCPTools(ctx context.Context, a *app.App) {
	go func() {
		defer logging.RecoverPanic("MCP-goroutine", nil)
		ctxWithTimeout, cancel := context.WithTimeout(ctx, 30*time.Second)
		defer cancel()
		agent.GetMcpTools(ctxWithTimeout, a.Permissions)
		logging.Info("MCP message handling goroutine exiting")
	}()
}
