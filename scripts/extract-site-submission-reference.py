#!/usr/bin/env python3
"""Render pinned public templates with owned data, never original controllers.

PHP_BINARY may select an installed PHP CLI. No source assets or scripts are fetched.
The profile values below are synthetic inputs, not recovered production settings.
"""
import argparse
import hashlib
from html.parser import HTMLParser
import json
import os
from pathlib import Path
import subprocess

PINS = {
    'banner-contest.tpl.php': 'f3c5a8ca8754c8be1cbec7f12787b23472e82e05525c80ba3ebb4f06738b2618',
    'banner-contest-vtube.tpl.php': '1798e3d2df94c907c8c1cace025af7606976704e8effcc84bf9145036c57df90',
    'banner-contest-delete.tpl.php': 'fb6cc09be7f8c6b5b197d78386489f9f6d4f4023a021112a5c37abb896eabba9',
    'banner-contest-delete-vtube.tpl.php': '39e076405f4bcf9d300615a86e56e9efc26ea8d7da5933a2d2432ea62e6674ef',
    'banner-contest-sent.tpl.php': '061a16ef193384f3d1351af75aeee2c626b9e4063c33c25b3ed5c26e8644ad88',
    'banner-contest-sent-vtube.tpl.php': 'da88fdd4a2b356fa4efa79dd933e12b4007f0ed868cc637aee8fbe1f483f0bbc',

    'feedback-review.tpl.php': '1c28a1fe99c82afe1b2ffd9f42bf9f6c78b36639860fca0119823d9a698fa493',
    'feedback.tpl.php': '92e6fb0550f333f584b97c4a7eb70dafe247a0e46feb94f5ab4a1586151df80e',
    'janitorapp.tpl.php': '35d172f0b7085162fa9b91b427fcf9beab2c07aa93175c6e9e74da4f1c4ac525',
    'janitorapp-success.tpl.php': '27871c487bd2ec714aebc58ff7bb6ea2782f2716848e4c00eadd0fa1123dee67',
}
FOOTER = "include '/www/4chan.org/web/www/frontpage_footer.php';"
HARNESS = r'''<?php
error_reporting(E_ALL);
set_error_handler(function($severity, $message) { throw new RuntimeException($message); });
date_default_timezone_set('UTC');
$c = json_decode($argv[1], true, 512, JSON_THROW_ON_ERROR);
define('IN_APP', true);
define('IS_4CHANNEL', $c['channel']);
define('APPLICATIONS_OPEN', $c['open'] ?? false);
define('WEB_DOMAIN', 'example.test');
$_SERVER = ['REQUEST_TIME' => 1704067200];
$_GET = isset($c['answered']) ? ['answered' => $c['answered']] : [];
function has_level($level) { global $c; return $level === 'admin' && ($c['admin'] ?? false); }
function csrf_tag() { return '<input name=owned_csrf value=owned>'; }
function captcha_form($interactive = false) { return '<div data-owned-captcha="1"></div>'; }
class OwnedSubmissionView {
    const WEB_ROOT = '/janitorapp';
    const DATE_FORMAT_SHORT = 'Y-m-d';
    const ERR_NOT_DESKTOP = 'Owned desktop-required message';
    const MAX_FIELD_LENGTH = 150;
    const MAX_TXT_FIELD_LENGTH = 2000;
    const STR_Q1 = 'Owned question 1';
    const STR_Q2 = 'Owned question 2';
    const STR_Q3 = 'Owned question 3';
    const STR_Q4 = 'Owned question 4';
    public $mode, $need_auth_email, $application, $app_uid;
    public $board, $submit_boards, $vote_boards, $view_boards, $votable, $items;
    public $_event_types = [1=>'Owned submission', 2=>'Owned voting'];
    public $_tkn = 'owned-csrf-token', $private_id = 'owned-private-id';
    public $categories = [1=>'General', 2=>'Technical'], $current_cat = null;
    public $cat_count = [1=>1, 2=>0], $draft_count = 1;
    public $is_draft_mode, $can_dismiss, $suggestions, $suggestion, $reply;
    public $replies = [], $reply_labels = ['admin'=>'Administrator', 'mod'=>'Moderator'];
    public $reply_icons = [], $capcodes = ['admin', 'mod'];
    public $has_next_page = false, $get_params = '';
    public $errstr = ['no_results'=>'Owned no results'];
    public $auth_uid = 'owned-edit-id', $auth_email = 'owned@example.test';
    public $unique_id = 'owned-edit-id';
    public $categoriesLabels = [1 => 'Owned general', 2 => 'Owned technical'];
    public $max_len_subject = 150, $max_len_message = 2000;
    public $message = 'Owned response';
    public $tz_names = [], $valid_boards = ['g' => true, 'a' => true];
    public function __construct(private array $case) {
        $this->mode = $case['mode'] ?? 'index';
        $this->board = $case['board'] ?? null;
        $this->submit_boards = ($case['submitting'] ?? false) ? ['g'=>'Owned technology'] : [];
        $this->vote_boards = ($case['votable'] ?? false) ? ['g'=>'Owned technology'] : [];
        $this->view_boards = [];
        $this->votable = $case['votable'] ?? false;
        $this->items = ($case['empty'] ?? false) ? [] : ($this->board ? [[
            'id'=>7, 'board'=>$this->board, 'file_id'=>'owned', 'file_ext'=>'png',
            'score'=>9, 'score2'=>8, 'width'=>468, 'height'=>60, 'th_width'=>0, 'th_height'=>0
        ]] : [['boards'=>'g,a', 'event_type'=>1, 'starts_on'=>1704067200, 'ends_on'=>1704153600]]);
        $this->is_draft_mode = ($case['answered'] ?? null) === 3;
        $this->can_dismiss = $case['can_dismiss'] ?? false;
        $this->suggestion = ['id'=>7, 'category'=>1, 'subject'=>'Owned subject',
            'message'=>'Owned message', 'updated_capcode'=>'none', 'updated_ts'=>1704067200,
            'updated_date'=>'2024-01-01', 'created_on'=>'2024-01-01', 'ip'=>'192.0.2.1',
            'extra'=>['http_ua'=>'Owned agent', 'http_lang'=>'en', '4chan_pass'=>null], 'geo'=>[]];
        $this->suggestions = ($case['empty'] ?? false) ? [] : [$this->suggestion];
        $this->reply = null;
        $this->need_auth_email = $case['need_email'] ?? false;
        $this->app_uid = ($case['existing'] ?? false) ? 'owned-edit-id' : null;
        $this->application = $this->app_uid ? [
            'firstname'=>'Owned <name>', 'handle'=>'Owned & handle',
            'email'=>'owned@example.test', 'age'=>25, 'tz'=>-4, 'hours'=>3,
            'times'=>'16:00-19:00', 'board1'=>'g', 'board2'=>'a',
            'q1'=>'Owned <answer>', 'q2'=>'Owned & answer',
            'q3'=>'Owned answer 3', 'q4'=>'Owned answer 4'] : null;
    }
    public function is_desktop_browser() { return $this->case['desktop'] ?? true; }
    public function get_csrf_token() { return 'owned-csrf-token'; }
    public function get_image_url($image, $thumb = false) { return '/owned-image.png'; }
    public function generate_token() { return 'owned-csrf-token'; }
    public function render($source) { eval('?>' . $source); }
}
(new OwnedSubmissionView($c))->render(base64_decode($argv[2], true));
'''

class Projection(HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.forms = []
        self.current = None
        self.select = None
        self.links = []
        self.meta = []
        self.actions = []
        self.score_columns = 0
        self.vote_buttons = []
        self.closed = False
        self.desktop = False

    def handle_starttag(self, tag, attrs):
        attr = dict(attrs)
        if tag == 'th' and attr.get('class') == 'col-score':
            self.score_columns += 1
        if tag == 'button' and attr.get('data-cmd') == 'pre-vote':
            self.vote_buttons.append(attr)
        if tag == 'form':
            self.current = {'attributes': attr, 'fields': []}
            self.forms.append(self.current)
        elif tag in ('input', 'textarea', 'select', 'button') and self.current is not None:
            field = {'tag': tag, **attr}
            self.current['fields'].append(field)
            if tag == 'select':
                field['options'] = []
                self.select = field
        elif tag == 'option' and self.select is not None:
            self.select['options'].append(attr)
        elif tag == 'a' and any(c in attr.get('class', '').split() for c in ('approve-link', 'dismiss-link')):
            self.actions.append(attr)
        elif tag == 'a' and 'owned-edit-id' in attr.get('href', ''):
            self.links.append(attr['href'])
        elif tag == 'meta' and attr.get('name') == 'referrer':
            self.meta.append(attr)

    def handle_endtag(self, tag):
        if tag == 'form':
            self.current = None
        if tag == 'select':
            self.select = None

    def handle_data(self, data):
        self.closed |= 'We are not accepting applications this time.' in data
        self.desktop |= 'Owned desktop-required message' in data

    def result(self):
        return {'score_columns': self.score_columns, 'vote_buttons': self.vote_buttons, 'actions': self.actions, 'forms': self.forms, 'private_links': self.links,
                'referrer': self.meta, 'closed': self.closed,
                'desktop_required': self.desktop}


def main():
    import base64
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path, nargs='?')
    parser.add_argument('--output', type=Path, default=Path(__file__).resolve().parents[1] / 'fixtures/site-submission-reference.json')
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    sources = {}
    original = {}
    saved = json.loads(args.output.read_text()) if args.source is None and args.check else None
    if args.source is None and not args.check:
        raise SystemExit('A source directory is required to regenerate')
    for filename, pin in PINS.items():
        raw = ((args.source / 'www.4chan' / 'views' / filename).read_bytes() if args.source
               else saved['templates'][filename].encode())
        if hashlib.sha256(raw).hexdigest() != pin:
            raise SystemExit(f'Audited source differs: {filename}')
        body = raw.decode()
        original[filename] = body
        expected = 0 if filename in ('janitorapp-success.tpl.php', 'feedback-review.tpl.php') else 1
        if body.count(FOOTER) != expected:
            raise SystemExit('Footer boundary differs')
        # The only external include is replaced with an owned empty footer.
        sources[filename] = body.replace(FOOTER, '$frontpage_footer = [];')
    cases = []
    for channel in (False, True):
        for mode in ('index', 'error', 'success'):
            cases.append({'template': 'feedback.tpl.php', 'channel': channel, 'mode': mode})
        for opened in (False, True):
            for desktop in (False, True):
                for need_email in (False, True):
                    for existing in (False, True):
                        cases.append({'template': 'janitorapp.tpl.php', 'channel': channel,
                                      'open': opened, 'desktop': desktop,
                                      'need_email': need_email, 'existing': existing})
        cases.append({'template': 'janitorapp-success.tpl.php', 'channel': channel})
    for mode in ('index', 'read'):
        for answered in (None, 1, 2, 3):
            for admin in (False, True):
                for dismiss in (False, True):
                    for empty in (False, True):
                        case = {'template': 'feedback-review.tpl.php', 'channel': False,
                                'mode': mode, 'admin': admin, 'can_dismiss': dismiss, 'empty': empty}
                        if answered is not None:
                            case['answered'] = answered
                        cases.append(case)
    for template in ('banner-contest.tpl.php', 'banner-contest-vtube.tpl.php'):
        board = 'all' if 'vtube' in template else 'g'
        for listing in (False, True):
            for submitting in (False, True):
                for votable in (False, True):
                    for empty in (False, True):
                        cases.append({'template': template, 'channel': False,
                                      'board': board if listing else None, 'submitting': submitting,
                                      'votable': votable, 'empty': empty})
    for template in PINS:
        if template.startswith('banner-contest-') and ('delete' in template or 'sent' in template):
            cases.append({'template': template, 'channel': False})
    results = []
    for case in cases:
        source = base64.b64encode(sources[case['template']].encode()).decode()
        run = subprocess.run([os.environ.get('PHP_BINARY', 'php'), '-n', '-d', 'allow_url_fopen=0', '-d', 'allow_url_include=0',
                              '-d', 'memory_limit=32M', '-r',
                              HARNESS.removeprefix('<?php'), json.dumps(case), source],
                             capture_output=True, text=True, check=True, timeout=10)
        if run.stderr:
            raise SystemExit(run.stderr)
        view = Projection()
        view.feed(run.stdout)
        results.append({'input': case, 'output': view.result()})
    document = {'source_sha256': PINS, 'templates': original,
                'profile': 'Owned synthetic categories, questions, limits, identities and tokens.',
                'cases': results}
    rendered = json.dumps(document, ensure_ascii=False, indent=2) + '\n'
    if args.check:
        if args.output.read_text() != rendered:
            raise SystemExit('Site-submission template fixture differs')
    else:
        args.output.write_text(rendered)
    print(f'{len(results)} isolated source submission template cases.')

if __name__ == '__main__':
    main()
