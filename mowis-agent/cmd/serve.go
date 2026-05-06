package cmd

import (
	"github.com/mowisai/mowis-agent/internal/server"
	"github.com/spf13/cobra"
)

var serveCmd = &cobra.Command{
	Use:   "serve",
	Short: "Start the HTTP API server",
	Long:  `Start the MowisAI agent HTTP API server for the desktop app and CLI.`,
	RunE:  serveRun,
}

func serveRun(cmd *cobra.Command, args []string) error {
	flags := parseFlags(cmd)

	port, _ := cmd.Flags().GetInt("port")
	hostname, _ := cmd.Flags().GetString("hostname")

	a, cwd, cancel, err := initApp(flags)
	if err != nil {
		return err
	}
	defer cancel()
	defer a.Shutdown()

	return server.Start(cmd.Context(), a, cwd, hostname, port)
}

func init() {
	serveCmd.Flags().IntP("port", "", 4096, "HTTP server port")
	serveCmd.Flags().StringP("hostname", "", "127.0.0.1", "HTTP server hostname")
	rootCmd.AddCommand(serveCmd)
}
