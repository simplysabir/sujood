use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "sujood", version, author, about = "A beautiful terminal companion for Islamic practice tracking")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// First-run setup wizard (location, calculation method, madhab)
    Setup {
        /// Reset existing configuration
        #[arg(long)]
        reset: bool,
    },
    /// Show today's prayer times and countdown to next prayer
    Times,
    /// Mark a prayer as done or missed
    Mark {
        /// Prayer name (fajr, zuhr, asr, maghrib, isha)
        prayer: String,
        /// Mark as missed and add to qada queue
        #[arg(long)]
        missed: bool,
    },
    /// Qada queue management
    Qada {
        #[command(subcommand)]
        action: QadaCommands,
    },
    /// Dhikr tracking
    Dhikr {
        #[command(subcommand)]
        action: DhikrCommands,
    },
    /// Log Quran pages read today
    Quran {
        /// Number of pages read
        pages: f64,
    },
    /// Show statistics
    Stats {
        /// Show ASCII heatmap for the last 7 days
        #[arg(long)]
        week: bool,
    },
    /// Export a weekly text summary to stdout
    Export,
}

#[derive(Subcommand, Debug)]
pub enum QadaCommands {
    /// Show the qada queue
    List,
    /// Mark the oldest qada prayer as completed
    Complete,
    /// Manually add a prayer to the qada queue
    Add {
        /// Prayer name
        prayer: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum DhikrCommands {
    /// Mark morning adhkar as done
    Morning,
    /// Mark evening adhkar as done
    Evening,
    /// Toggle or increment a dhikr by name
    Mark {
        /// Dhikr name
        name: String,
        /// Add this count to a counter dhikr
        #[arg(long)]
        count: Option<i32>,
    },
    /// Add a custom dhikr
    Add {
        /// Dhikr name
        name: String,
        /// Type: checkbox or counter
        #[arg(long, default_value = "checkbox")]
        r#type: String,
        /// Target count (for counter type)
        #[arg(long, default_value = "1")]
        target: i32,
        /// Frequency: daily or weekly
        #[arg(long, default_value = "daily")]
        freq: String,
    },
    /// List all active dhikr definitions
    List,
}
