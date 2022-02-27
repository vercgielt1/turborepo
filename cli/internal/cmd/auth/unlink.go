package auth

import (
	"path/filepath"

	"github.com/spf13/cobra"
	"github.com/vercel/turborepo/cli/internal/cmdutil"
	"github.com/vercel/turborepo/cli/internal/config"
	"github.com/vercel/turborepo/cli/internal/util"
)

func UnlinkCmd(ch *cmdutil.Helper) *cobra.Command {
	cmd := &cobra.Command{
		Use:   "unlink",
		Short: "Unlink the current directory from your Vercel organization and disable Remote Caching (beta)",
		RunE: func(cmd *cobra.Command, args []string) error {
			if err := config.WriteConfigFile(filepath.Join(".turbo", "config.json"), &config.TurborepoConfig{}); err != nil {
				return ch.LogError("could not unlink. Something went wrong: %w", err)
			}

			ch.Logger.Printf(util.Sprintf("${GREY}> Disabled Remote Caching${RESET}"))
			return nil
		},
	}

	return cmd
}
