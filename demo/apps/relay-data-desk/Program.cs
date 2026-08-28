using System.Drawing.Drawing2D;
using System.Windows.Forms;

namespace RelayDataDesk;

internal static class Program
{
    [STAThread]
    private static void Main()
    {
        ApplicationConfiguration.Initialize();
        Application.Run(new MainForm());
    }
}

internal sealed class MainForm : Form
{
    private readonly Panel content;
    private readonly Label status;
    private readonly TextBox username;
    private readonly TextBox password;
    private readonly Button loginButton;
    private Button? fetchButton;
    private Button? copyButton;
    private Label? dataLabel;

    private static readonly Color Ink = Color.FromArgb(235, 239, 238);
    private static readonly Color Muted = Color.FromArgb(157, 170, 168);
    private static readonly Color PanelColor = Color.FromArgb(28, 34, 34);
    private static readonly Color PanelAlt = Color.FromArgb(35, 43, 42);
    private static readonly Color Teal = Color.FromArgb(52, 183, 166);
    private static readonly Color Orange = Color.FromArgb(226, 135, 54);

    public MainForm()
    {
        Text = "Relay Data Desk";
        ClientSize = new Size(760, 500);
        MinimumSize = Size;
        MaximumSize = Size;
        StartPosition = FormStartPosition.CenterScreen;
        BackColor = Color.FromArgb(18, 23, 23);
        ForeColor = Ink;
        Font = new Font("Segoe UI", 10F);

        var header = new Panel { Dock = DockStyle.Top, Height = 78, BackColor = Color.FromArgb(24, 30, 30), Padding = new Padding(28, 16, 28, 10) };
        header.Paint += (_, e) =>
        {
            using var pen = new Pen(Color.FromArgb(45, 61, 59));
            e.Graphics.DrawLine(pen, 28, header.Height - 1, header.Width - 28, header.Height - 1);
        };
        header.Controls.Add(new Label { Text = "RELAY DATA DESK", AutoSize = true, Font = new Font("Segoe UI Semibold", 16F), ForeColor = Ink, Location = new Point(28, 15) });
        header.Controls.Add(new Label { Text = "Local order source  /  demo workspace", AutoSize = true, Font = new Font("Segoe UI", 9F), ForeColor = Muted, Location = new Point(30, 44) });
        Controls.Add(header);

        status = new Label { Dock = DockStyle.Bottom, Height = 34, Text = "Waiting for sign in", TextAlign = ContentAlignment.MiddleLeft, Padding = new Padding(28, 0, 0, 0), ForeColor = Muted, BackColor = Color.FromArgb(24, 30, 30) };
        Controls.Add(status);

        content = new Panel { Dock = DockStyle.Fill, Padding = new Padding(48, 34, 48, 28), BackColor = Color.FromArgb(18, 23, 23) };
        Controls.Add(content);

        username = TextBox("", false);
        password = TextBox("", true);
        loginButton = Button("SIGN IN", Orange);
        loginButton.Click += (_, _) => SignIn();
        password.KeyDown += (_, e) => { if (e.KeyCode == Keys.Enter) { e.SuppressKeyPress = true; SignIn(); } };

        var loginPanel = Card(310, 310);
        loginPanel.Controls.Add(Label("SIGN IN TO DATA SOURCE", 18, 18, 18, Ink));
        loginPanel.Controls.Add(Label("Use the demo account to unlock the latest order record.", 18, 50, 9, Muted));
        loginPanel.Controls.Add(Label("USERNAME", 18, 93, 8, Muted));
        username.Location = new Point(18, 111);
        username.Width = 274;
        loginPanel.Controls.Add(username);
        loginPanel.Controls.Add(Label("PASSWORD", 18, 154, 8, Muted));
        password.Location = new Point(18, 172);
        password.Width = 274;
        loginPanel.Controls.Add(password);
        loginButton.Location = new Point(18, 226);
        loginButton.Width = 274;
        loginPanel.Controls.Add(loginButton);
        loginPanel.Controls.Add(Label("demo  /  demo1234", 18, 273, 8, Muted));
        content.Controls.Add(loginPanel);
        CenterPanel(loginPanel);
        content.Resize += (_, _) =>
        {
            if (content.Controls.Count > 0) CenterPanel(content.Controls[0]);
        };
        username.Focus();
    }

    private void SignIn()
    {
        if (username.Text != "demo" || password.Text != "demo1234")
        {
            status.Text = "Sign in failed  ·  check the demo credentials";
            status.ForeColor = Color.FromArgb(232, 112, 100);
            return;
        }

        content.Controls.Clear();
        status.Text = "Authenticated  ·  ready to fetch one order";
        status.ForeColor = Teal;

        var panel = Card(640, 310);
        panel.Controls.Add(Label("DATA SOURCE CONNECTED", 22, 18, 18, Ink));
        panel.Controls.Add(Label("The desktop source is authenticated. Fetch a record to continue.", 22, 52, 9, Muted));
        fetchButton = Button("FETCH LATEST ORDER", Teal);
        fetchButton.Location = new Point(22, 92);
        fetchButton.Width = 270;
        fetchButton.Click += (_, _) => FetchOrder(panel);
        panel.Controls.Add(fetchButton);
        panel.Controls.Add(Label("SOURCE STATUS", 22, 151, 8, Muted));
        panel.Controls.Add(Label("Connected to local demo data", 22, 170, 11, Teal));
        content.Controls.Add(panel);
        CenterPanel(panel);
        fetchButton.Focus();
    }

    private void CenterPanel(Control panel)
    {
        panel.Left = Math.Max(0, (content.ClientSize.Width - panel.Width) / 2);
        panel.Top = Math.Max(0, (content.ClientSize.Height - panel.Height) / 2);
    }

    private void FetchOrder(Panel panel)
    {
        fetchButton!.Enabled = false;
        fetchButton.Text = "FETCHING...";
        status.Text = "Fetching latest order record...";
        status.ForeColor = Orange;
        var timer = new System.Windows.Forms.Timer { Interval = 700 };
        timer.Tick += (_, _) =>
        {
            timer.Stop();
            timer.Dispose();
            dataLabel = Label("ORDER  #RLY-2048\nCustomer     Aster Works\nItem         Wireless keyboard\nQuantity     3\nTotal        ¥12,800\nStatus       READY FOR RELAY", 22, 145, 12, Ink);
            dataLabel.Font = new Font("Consolas", 11F);
            dataLabel.AutoSize = true;
            panel.Controls.Add(dataLabel);
            copyButton = Button("COPY DATA FOR RELAY", Orange);
            copyButton.Location = new Point(330, 92);
            copyButton.Width = 270;
            copyButton.Click += (_, _) => CopyOrder();
            panel.Controls.Add(copyButton);
            status.Text = "Record loaded  ·  ready for Relay to copy";
            status.ForeColor = Teal;
            fetchButton.Text = "ORDER LOADED";
        };
        timer.Start();
    }

    private void CopyOrder()
    {
        Clipboard.SetText("ORDER_ID=RLY-2048;CUSTOMER=Aster Works;ITEM=Wireless keyboard;QUANTITY=3;TOTAL=12800;STATUS=READY FOR RELAY");
        copyButton!.Text = "DATA COPIED";
        copyButton.Enabled = false;
        status.Text = "Data copied to clipboard  ·  hand off to the Canvas app";
        status.ForeColor = Teal;
    }

    private static TextBox TextBox(string value, bool secret) => new()
    {
        Text = value,
        BorderStyle = BorderStyle.FixedSingle,
        BackColor = PanelAlt,
        ForeColor = Ink,
        Font = new Font("Segoe UI", 10F),
        UseSystemPasswordChar = secret,
        Height = 30,
    };

    private static Button Button(string text, Color color) => new()
    {
        Text = text,
        FlatStyle = FlatStyle.Flat,
        FlatAppearance = { BorderColor = color, BorderSize = 1 },
        BackColor = color,
        ForeColor = Color.FromArgb(15, 22, 21),
        Font = new Font("Segoe UI Semibold", 9F),
        Height = 34,
        Cursor = Cursors.Hand,
        TabStop = true,
    };

    private static Label Label(string text, int x, int y, float size, Color color) => new()
    {
        Text = text,
        AutoSize = true,
        Location = new Point(x, y),
        ForeColor = color,
        Font = new Font("Segoe UI", size),
    };

    private static Panel Card(int width, int height) => new()
    {
        Size = new Size(width, height),
        BackColor = PanelColor,
        BorderStyle = BorderStyle.FixedSingle,
        Anchor = AnchorStyles.None,
        Location = new Point(0, 0),
    };
}
