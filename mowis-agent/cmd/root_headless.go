package cmd

import (
	"context"
	"fmt"
	"os"

	"github.com/mowisai/mowis-agent/internal/format"
	"github.com/mowisai/mowis-agent/internal/version"
	"github.com/spf13/cobra"
)

var rootCmd = &cobra.Command{
	Use:   "mowis-agent",
	Short: "MowisAI coding agent backend",
	Long:  `MowisAI coding agent — serves the HTTP API for the desktop app and CLI.`,
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

		// Non-interactive prompt mode
		if flags.Prompt != "" {
			a, _, cancel, err := initApp(flags)
			if err != nil {
				return err
			}
			defer cancel()
			defer a.Shutdown()
			return a.RunNonInteractive(context.Background(), flags.Prompt, flags.OutputFormat, flags.Quiet)
		}

		// Default to serve mode in headless builds
		return serveRun(cmd, args)
	},
}

func Execute() {
	if err := rootCmd.Execute(); err != nil {
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
	rootCmd.Flags().IntP("port", "", 4096, "HTTP server port (headless mode)")
	rootCmd.Flags().StringP("hostname", "", "127.0.0.1", "HTTP server hostname")

	rootCmd.RegisterFlagCompletionFunc("output-format", func(cmd *cobra.Command, args []string, toComplete string) ([]string, cobra.ShellCompDirective) {
		return format.SupportedFormats, cobra.ShellCompDirectiveNoFileComp
	})
}
